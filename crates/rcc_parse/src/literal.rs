//! Phase-7 literal decoders.
//!
//! Converts a pp-number / char-constant / string-literal text slice into
//! the typed `*Literal` payload carried by a parser-level `Token`. For now
//! only `decode_integer` is wired (task 05-03); float/char/string decoders
//! follow in tasks 05-04 / 05-05 / 05-06.
//!
//! The decoders take a plain `&str` (the source slice covered by the
//! pp-token) and return a `Result<_, Diagnostic>`. The returned diagnostic
//! is **spanless** — the caller attaches the pp-token's span as the primary
//! label and hands it to the session `Handler`. Keeping the span off the
//! decoder's concern means the function is trivially unit-testable without
//! a `Session` or `SourceMap` in scope.
//!
//! C99 reference: §6.4.4.1 "Integer constants".

use rcc_errors::{codes, Diagnostic, Level};

use crate::token::{FloatLiteral, FloatSuffix, IntLiteral, IntSuffix};

/// Decode a `PpNumberKind::Integer` source slice into an [`IntLiteral`].
///
/// Accepts decimal (`42`), octal (`0777`), and hexadecimal (`0xFF`)
/// forms, with any legal C99 integer suffix (`u`, `l`, `ll`, `ul`, `ull`)
/// in either pair order and in any combination of casing — with the sole
/// exception that the two letters of `long long` must share a case
/// (C99 §6.4.4.1: the *long-long-suffix* is literally `ll` or `LL`, not
/// `lL` or `Ll`).
///
/// The function does **not** perform final type selection (C99 §6.4.4.1p5
/// ladder) — that lives in typeck task 07-01. `IntSuffix` here records
/// only the explicit suffix family the programmer wrote.
///
/// # Errors
///
/// Returns a spanless [`Diagnostic`] for:
///
/// - **E0040** `integer literal too large` — magnitude overflows `u128`.
///   Chosen over E0009 (lexer) because the lexer sees the pp-number
///   grammar only and cannot check magnitude.
/// - **E0011** `invalid octal digit` — a literal with the octal prefix
///   (leading `0`) contains the digit `8` or `9`. Shares the code with
///   the lexer-side check so tooling sees a single family.
/// - No code — digit-separator apostrophes (C++14 feature, never C99),
///   stray characters in the digit run, and malformed suffixes. These
///   cases are defensive: the lexer normally never produces a pp-number
///   carrying an apostrophe because `'` in C99 opens a character
///   constant, but `decode_integer` runs on pre-validated text and
///   must still refuse it cleanly rather than misinterpret it.
///
/// The caller (`phase7::pp_to_token`) attaches the pp-token span as the
/// primary label before emitting the diagnostic via `Session::handler`.
pub fn decode_integer(text: &str) -> Result<IntLiteral, Diagnostic> {
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return Err(plain_err("empty integer literal"));
    }

    // A digit separator is never legal in C99 (§6.4.4.1 does not list
    // `'` anywhere in the integer-constant grammar). Reject it up front
    // so the subsequent octal check does not misread `0'8` as "leading
    // zero followed by digit 8" → "invalid octal digit".
    if bytes.contains(&b'\'') {
        return Err(plain_err("digit separators in integer literals are a C++14 feature, not C99"));
    }

    let (base, digit_start) = classify_base(bytes);

    // Walk digits. Base-matching characters advance `value`; the first
    // suffix letter (u/U/l/L) breaks the loop cleanly; anything else is
    // a malformed literal.
    let mut value: u128 = 0;
    // For the octal family the leading `0` itself is a valid digit, so
    // `"0"` alone has `saw_any_digit == true`. Decimal and hex start
    // `false` because their digit run has not begun yet.
    let mut saw_any_digit = base == 8;
    let mut i = digit_start;
    while i < bytes.len() {
        let b = bytes[i];
        let digit_val = match b {
            b'0'..=b'9' => u32::from(b - b'0'),
            b'a'..=b'f' => u32::from(b - b'a') + 10,
            b'A'..=b'F' => u32::from(b - b'A') + 10,
            b'u' | b'U' | b'l' | b'L' => break,
            _ => {
                return Err(plain_err(format!(
                    "unexpected character `{}` in integer literal",
                    b as char
                )));
            }
        };
        if digit_val >= base {
            // Only reachable for base=8 with '8' or '9', or for a hex
            // digit 'a'..='f' in a decimal literal. The former is the
            // spec-cited "invalid octal digit" case; the latter is a
            // float's exponent letter (`1e2`) that the lexer should
            // have tagged as PpNumberKind::Float — if we see it here
            // the pp-tokeniser is out of sync and we treat it as a
            // general malformed integer.
            if base == 8 {
                return Err(coded_err(
                    codes::E0011,
                    format!("invalid digit `{}` in octal literal", b as char),
                ));
            } else {
                return Err(plain_err(format!(
                    "invalid digit `{}` in decimal integer literal",
                    b as char
                )));
            }
        }
        value = value
            .checked_mul(u128::from(base))
            .and_then(|v| v.checked_add(u128::from(digit_val)))
            .ok_or_else(|| coded_err(codes::E0040, "integer literal too large"))?;
        saw_any_digit = true;
        i += 1;
    }

    if !saw_any_digit {
        // `"0x"` / `"0X"` with no hex digits. The lexer's pp-number
        // grammar would normally swallow the leading letter too, but
        // guard against malformed callers regardless.
        return Err(plain_err("hex integer literal has no digits"));
    }

    let suffix = parse_suffix(&bytes[i..])?;
    Ok(IntLiteral { value, suffix })
}

/// Determine the radix and the starting index of the digit run.
///
/// `0x` / `0X` → hex (skip the two-character prefix). A bare leading
/// `0` (not followed by `x`/`X`) is octal; the `0` itself is the first
/// digit, so the caller consumes it as value 0. Everything else is
/// decimal starting at index 0.
fn classify_base(bytes: &[u8]) -> (u32, usize) {
    if bytes.first() == Some(&b'0') {
        if matches!(bytes.get(1), Some(b'x') | Some(b'X')) {
            (16, 2)
        } else {
            // `"0"` alone and `"0777"` both enter here; the leading
            // zero is the first octal digit (value 0).
            (8, 1)
        }
    } else {
        (10, 0)
    }
}

/// Parse the integer suffix tail (the bytes *after* the digit run).
///
/// The C99 §6.4.4.1 integer-suffix grammar is:
///
/// ```text
/// integer-suffix:
///     unsigned-suffix long-suffix?
///     unsigned-suffix long-long-suffix
///     long-suffix unsigned-suffix?
///     long-long-suffix unsigned-suffix?
/// unsigned-suffix:     u | U
/// long-suffix:         l | L
/// long-long-suffix:    ll | LL
/// ```
///
/// So `u` / `U` may appear at most once; `l`/`L` or `ll`/`LL` may appear
/// at most once; `lL` and `Ll` are forbidden because `long-long-suffix`
/// is defined as two identical letters. Ordering between the `u`-group
/// and the `l`-group is free (`lu` and `ul` are both legal).
fn parse_suffix(s: &[u8]) -> Result<IntSuffix, Diagnostic> {
    if s.is_empty() {
        return Ok(IntSuffix::None);
    }
    for &b in s {
        if !matches!(b, b'u' | b'U' | b'l' | b'L') {
            return Err(plain_err(format!("invalid integer suffix character `{}`", b as char)));
        }
    }

    let mut has_u = false;
    // 0 = none, 1 = single l/L, 2 = ll/LL.
    let mut l_count: u8 = 0;
    let mut i = 0;
    while i < s.len() {
        let b = s[i];
        match b {
            b'u' | b'U' => {
                if has_u {
                    return Err(plain_err("`u`/`U` suffix given more than once"));
                }
                has_u = true;
                i += 1;
            }
            b'l' | b'L' => {
                if l_count > 0 {
                    return Err(plain_err("long-suffix given more than once"));
                }
                // Look for a paired ll/LL (identical case).
                if s.get(i + 1) == Some(&b) {
                    l_count = 2;
                    i += 2;
                } else if matches!(s.get(i + 1), Some(&b'l') | Some(&b'L')) {
                    // `lL` or `Ll` — mixed case, forbidden.
                    return Err(plain_err(
                        "`long long` suffix must use the same case twice (`ll` or `LL`)",
                    ));
                } else {
                    l_count = 1;
                    i += 1;
                }
            }
            _ => unreachable!("filtered by the character check above"),
        }
    }

    Ok(match (has_u, l_count) {
        (false, 0) => IntSuffix::None,
        (true, 0) => IntSuffix::U,
        (false, 1) => IntSuffix::L,
        (false, 2) => IntSuffix::LL,
        (true, 1) => IntSuffix::UL,
        (true, 2) => IntSuffix::ULL,
        // (_, n) with n > 2 is unreachable: `l_count` is only set to 0,
        // 1, or 2 above.
        _ => unreachable!(),
    })
}

/// Decode a `PpNumberKind::Float` source slice into a [`FloatLiteral`].
///
/// Accepts the two C99 §6.4.4.2 floating-constant families:
///
/// - **Decimal** floats like `1.0`, `.5e10`, `3.14e-10f`, `2.0L`. The
///   mantissa/exponent grammar is delegated to [`f64::from_str`], which
///   accepts exactly the productions the C99 decimal-floating-constant
///   grammar describes (modulo the suffix, which we strip first).
/// - **Hex** floats like `0x1.0p0`, `0x1.8p1`, `0x1.0p3`. The binary
///   exponent `p` / `P` is **required** by the grammar even when the
///   value would be exact; a spelling like `0x1.0` with no `p` is a
///   constraint violation and is rejected here.
///
/// `long double` (`l` / `L` suffix) is recorded in [`FloatSuffix::L`]
/// but its *value* is still stored in an `f64` — `rcc` does not model
/// 80-bit/128-bit extended precision. This is a deliberate fidelity
/// trade-off: codegen will later widen the stored `f64` to whatever
/// type the target's `long double` maps to, accepting the precision
/// loss on the (rare) literals whose extra digits would have survived.
/// Full `f128` arithmetic is future work.
///
/// Overflow (magnitude beyond `f64::MAX`) is **not** an error. The
/// value is returned as `±infinity` and the caller is expected to emit
/// [`codes::W0002`] (`float literal overflow`) as a warning attached
/// to the literal's span. We signal this by returning `Ok` with an
/// infinite value — normal pp-number source text cannot spell infinity,
/// so `value.is_infinite()` after a successful decode is an unambiguous
/// overflow signal.
///
/// # Errors
///
/// Returns a spanless [`Diagnostic`] for:
///
/// - Malformed decimal mantissa / exponent (e.g. `1.0ff`, `1e`).
/// - Hex float with no digits, no `p` / `P` exponent, or trailing
///   junk (e.g. `0x1.0`, `0x1p`, `0x1.0p0q`).
/// - Invalid suffix (more than one of `f`/`F`/`l`/`L`, or a letter
///   outside that set).
pub fn decode_float(text: &str) -> Result<FloatLiteral, Diagnostic> {
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return Err(plain_err("empty floating literal"));
    }

    // Strip the trailing floating-suffix letter if present. C99
    // §6.4.4.2 permits exactly one of `f`/`F`/`l`/`L`, so a single
    // final letter is enough to disambiguate. For hex floats the
    // last mantissa character before any suffix is always a decimal
    // exponent digit (because the `p`/`P` exponent is mandatory and
    // its digit sequence is decimal), so `f`/`F` at the tail cannot
    // be mistaken for a hex digit.
    let (mantissa_bytes, suffix) = match bytes.last() {
        Some(&b'f') | Some(&b'F') => (&bytes[..bytes.len() - 1], FloatSuffix::F),
        Some(&b'l') | Some(&b'L') => (&bytes[..bytes.len() - 1], FloatSuffix::L),
        _ => (bytes, FloatSuffix::None),
    };

    // Reject a bare suffix with no mantissa (`"f"` etc.) up front so
    // the decimal / hex decoders never see an empty slice.
    if mantissa_bytes.is_empty() {
        return Err(plain_err("floating literal has no digits"));
    }

    let is_hex = mantissa_bytes.len() >= 2
        && mantissa_bytes[0] == b'0'
        && matches!(mantissa_bytes[1], b'x' | b'X');

    let value = if is_hex {
        parse_hex_float(mantissa_bytes)?
    } else {
        parse_decimal_float(mantissa_bytes)?
    };

    Ok(FloatLiteral { value, suffix })
}

/// Decode a decimal floating constant (minus any stripped suffix).
///
/// Defers to `f64::from_str`, whose grammar is a strict superset of
/// C99 §6.4.4.2 *decimal-floating-constant* (it additionally accepts
/// `inf`, `nan`, and a leading sign — none of which the lexer's
/// pp-number FSM will ever produce, so the superset is safe). Overflow
/// inside `from_str` surfaces as `Ok(f64::INFINITY)` rather than `Err`,
/// which matches the "overflow → `+∞` + warning" behavior the caller
/// wants; we propagate it transparently.
fn parse_decimal_float(bytes: &[u8]) -> Result<f64, Diagnostic> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| plain_err("non-UTF-8 bytes in floating literal"))?;
    text.parse::<f64>().map_err(|_| plain_err(format!("malformed floating literal `{text}`")))
}

/// Decode a hex floating constant (`0x` / `0X` prefix already present,
/// suffix already stripped).
///
/// Grammar (C99 §6.4.4.2):
///
/// ```text
/// hexadecimal-floating-constant:
///     hex-prefix hex-digit-sequence        binary-exponent-part
///     hex-prefix hex-digit-sequence '.'    binary-exponent-part
///     hex-prefix hex-digit-sequence? '.' hex-digit-sequence binary-exponent-part
/// binary-exponent-part:
///     ('p'|'P') sign? digit-sequence
/// ```
///
/// The binary exponent is **mandatory** — a spelling like `0x1.0` has
/// no `p` and is rejected here.
///
/// We assemble the mantissa as an `f64` directly rather than routing
/// through `f64::from_str` (stable `from_str` rejects hex floats) or
/// through `u64`-then-scale (which would lose low bits on long hex
/// mantissas). Precision is IEEE-754 double: the hex grammar gives at
/// most ~13 significant hex digits before the 53-bit mantissa runs
/// out, and we accept that rounding as part of the `f64` choice.
fn parse_hex_float(bytes: &[u8]) -> Result<f64, Diagnostic> {
    // The caller already checked the `0x`/`0X` prefix; skip it.
    let body = &bytes[2..];

    let mut i = 0;
    let mut mantissa: f64 = 0.0;
    let mut saw_digit = false;

    // Integer part.
    while i < body.len() {
        if let Some(d) = hex_digit_value(body[i]) {
            mantissa = mantissa * 16.0 + f64::from(d);
            saw_digit = true;
            i += 1;
        } else {
            break;
        }
    }

    // Optional fractional part.
    if i < body.len() && body[i] == b'.' {
        i += 1;
        let mut scale: f64 = 1.0 / 16.0;
        while i < body.len() {
            if let Some(d) = hex_digit_value(body[i]) {
                mantissa += f64::from(d) * scale;
                scale /= 16.0;
                saw_digit = true;
                i += 1;
            } else {
                break;
            }
        }
    }

    if !saw_digit {
        return Err(plain_err("hex floating literal has no digits"));
    }

    // Binary exponent — mandatory.
    if i >= body.len() || !matches!(body[i], b'p' | b'P') {
        return Err(plain_err("hex floating literal requires a `p`/`P` binary exponent"));
    }
    i += 1;

    // Optional exponent sign.
    let exp_sign: i32 = match body.get(i) {
        Some(&b'+') => {
            i += 1;
            1
        }
        Some(&b'-') => {
            i += 1;
            -1
        }
        _ => 1,
    };

    // Exponent digits (decimal, at least one).
    let mut exp_val: i64 = 0;
    let mut saw_exp_digit = false;
    while i < body.len() && body[i].is_ascii_digit() {
        exp_val = exp_val.saturating_mul(10).saturating_add(i64::from(body[i] - b'0'));
        saw_exp_digit = true;
        i += 1;
    }
    if !saw_exp_digit {
        return Err(plain_err("hex floating literal exponent has no digits"));
    }

    // Anything still unread is trailing junk (e.g. `0x1.0p0q`).
    if i != body.len() {
        return Err(plain_err(format!(
            "unexpected character `{}` in hex floating literal",
            body[i] as char
        )));
    }

    // `mantissa * 2^exp`. Clamping the exponent into `i32` before
    // `powi` means extreme spellings produce `±inf` / `0.0` rather
    // than UB; the caller treats `inf` as overflow (W0002).
    let signed_exp: i64 = i64::from(exp_sign) * exp_val;
    let clamped_exp: i32 = if signed_exp > i64::from(i32::MAX) {
        i32::MAX
    } else if signed_exp < i64::from(i32::MIN) {
        i32::MIN
    } else {
        signed_exp as i32
    };
    Ok(mantissa * 2f64.powi(clamped_exp))
}

/// Map `0..=9`, `a..=f`, `A..=F` to `0..=15`; anything else is `None`.
fn hex_digit_value(b: u8) -> Option<u32> {
    match b {
        b'0'..=b'9' => Some(u32::from(b - b'0')),
        b'a'..=b'f' => Some(u32::from(b - b'a') + 10),
        b'A'..=b'F' => Some(u32::from(b - b'A') + 10),
        _ => None,
    }
}

/// Build a spanless error diagnostic without a stable code.
fn plain_err(msg: impl Into<String>) -> Diagnostic {
    Diagnostic {
        level: Level::Error,
        code: None,
        message: msg.into(),
        labels: Vec::new(),
        notes: Vec::new(),
        help: Vec::new(),
    }
}

/// Build a spanless error diagnostic carrying a stable code.
fn coded_err(code: &'static str, msg: impl Into<String>) -> Diagnostic {
    let mut d = plain_err(msg);
    d.code = Some(code);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(text: &str) -> IntLiteral {
        decode_integer(text).unwrap_or_else(|e| panic!("decode {text:?} → error {:?}", e.message))
    }

    fn err(text: &str) -> Diagnostic {
        decode_integer(text).err().unwrap_or_else(|| panic!("decode {text:?} unexpectedly ok"))
    }

    // ── Happy-path decimal / zero ────────────────────────────────────

    #[test]
    fn decimal_zero_has_value_zero_and_no_suffix() {
        let lit = ok("0");
        assert_eq!(lit.value, 0);
        assert_eq!(lit.suffix, IntSuffix::None);
    }

    #[test]
    fn decimal_small() {
        let lit = ok("42");
        assert_eq!(lit.value, 42);
        assert_eq!(lit.suffix, IntSuffix::None);
    }

    #[test]
    fn decimal_large_but_within_u128() {
        // 2^100 is well inside u128 (max ≈ 3.4 × 10^38).
        let lit = ok("1267650600228229401496703205376");
        assert_eq!(lit.value, 1u128 << 100);
    }

    // ── Hex / octal base detection ───────────────────────────────────

    #[test]
    fn hex_lowercase() {
        let lit = ok("0xff");
        assert_eq!(lit.value, 0xff);
    }

    #[test]
    fn hex_mixed_case_prefix_and_digits() {
        let lit = ok("0XDeAdBeEf");
        assert_eq!(lit.value, 0xdead_beef);
    }

    #[test]
    fn octal_three_digits() {
        let lit = ok("0777");
        assert_eq!(lit.value, 0o777);
    }

    #[test]
    fn octal_leading_zero_with_no_further_digits_is_zero() {
        // A bare `0` is octal by the grammar but the value is the same.
        assert_eq!(ok("0").value, 0);
    }

    // ── Suffixes ────────────────────────────────────────────────────

    #[test]
    fn suffix_u_lower() {
        let lit = ok("1u");
        assert_eq!(lit.value, 1);
        assert_eq!(lit.suffix, IntSuffix::U);
    }

    #[test]
    fn suffix_ull_upper() {
        let lit = ok("42ULL");
        assert_eq!(lit.value, 42);
        assert_eq!(lit.suffix, IntSuffix::ULL);
    }

    #[test]
    fn suffix_ll_lower() {
        let lit = ok("7ll");
        assert_eq!(lit.suffix, IntSuffix::LL);
    }

    #[test]
    fn suffix_l_upper() {
        let lit = ok("9L");
        assert_eq!(lit.suffix, IntSuffix::L);
    }

    #[test]
    fn suffix_ul_and_lu_are_equivalent() {
        // Both orderings within the pair are legal (§6.4.4.1).
        assert_eq!(ok("42UL").suffix, IntSuffix::UL);
        assert_eq!(ok("42ul").suffix, IntSuffix::UL);
        assert_eq!(ok("42LU").suffix, IntSuffix::UL);
        assert_eq!(ok("42lu").suffix, IntSuffix::UL);
    }

    #[test]
    fn suffix_u_on_hex() {
        let lit = ok("0xFFu");
        assert_eq!(lit.value, 0xff);
        assert_eq!(lit.suffix, IntSuffix::U);
    }

    // ── Error: digit separators (C++ feature, not C99) ───────────────

    #[test]
    fn digit_separators_are_rejected_in_decimal() {
        let e = err("1'000'000");
        assert!(e.message.contains("digit separator"), "got: {}", e.message);
    }

    #[test]
    fn digit_separators_are_rejected_in_hex() {
        let e = err("0x1'000'000");
        assert!(e.message.contains("digit separator"), "got: {}", e.message);
    }

    // ── Error: invalid octal digit (leading 0 + 8/9) ─────────────────

    #[test]
    fn invalid_octal_digit_emits_e0011() {
        let e = err("0789");
        assert_eq!(e.code, Some(codes::E0011));
        assert!(e.message.contains("octal"));
    }

    // ── Error: overflow → E0040 ──────────────────────────────────────

    #[test]
    fn hex_overflow_u128_emits_e0040() {
        // 35 hex digits of F = 140 bits, well above u128's 128.
        let e = err("0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF");
        assert_eq!(e.code, Some(codes::E0040));
        assert!(e.message.contains("too large"), "got: {}", e.message);
    }

    #[test]
    fn decimal_overflow_u128_emits_e0040() {
        // u128::MAX = 340_282_366_920_938_463_463_374_607_431_768_211_455
        // One more than MAX overflows the accumulator.
        let e = err("340282366920938463463374607431768211456");
        assert_eq!(e.code, Some(codes::E0040));
    }

    #[test]
    fn u128_max_exactly_is_accepted() {
        // Boundary check: the maximum representable u128 must NOT
        // trigger E0040.
        let lit = ok("340282366920938463463374607431768211455");
        assert_eq!(lit.value, u128::MAX);
    }

    // ── Error: malformed suffix ──────────────────────────────────────

    #[test]
    fn mixed_case_long_long_is_rejected() {
        // `lL` / `Ll` are not spelled by the C99 long-long-suffix rule.
        assert!(decode_integer("1lL").is_err());
        assert!(decode_integer("1Ll").is_err());
    }

    #[test]
    fn duplicate_u_suffix_is_rejected() {
        assert!(decode_integer("1uu").is_err());
    }

    #[test]
    fn duplicate_long_suffix_is_rejected() {
        // Three l's — not a valid grouping.
        assert!(decode_integer("1lll").is_err());
        // ll + l
        assert!(decode_integer("1LLL").is_err());
    }

    #[test]
    fn junk_in_digit_run_is_rejected() {
        assert!(decode_integer("1@2").is_err());
    }

    #[test]
    fn empty_input_is_rejected() {
        assert!(decode_integer("").is_err());
    }

    // ── decode_float: happy-path decimal ─────────────────────────────

    fn fok(text: &str) -> FloatLiteral {
        decode_float(text)
            .unwrap_or_else(|e| panic!("decode_float {text:?} → error {:?}", e.message))
    }

    fn ferr(text: &str) -> Diagnostic {
        decode_float(text).err().unwrap_or_else(|| panic!("decode_float {text:?} unexpectedly ok"))
    }

    #[test]
    fn decimal_1_0_has_value_one() {
        let lit = fok("1.0");
        assert_eq!(lit.value, 1.0);
        assert_eq!(lit.suffix, FloatSuffix::None);
    }

    #[test]
    fn decimal_dot_5_e10() {
        let lit = fok(".5e10");
        assert_eq!(lit.value, 0.5e10);
        assert_eq!(lit.suffix, FloatSuffix::None);
    }

    #[test]
    fn decimal_3_14_e_neg10_with_f_suffix() {
        let lit = fok("3.14e-10f");
        assert!((lit.value - 3.14e-10).abs() < 1e-20, "got {}", lit.value);
        assert_eq!(lit.suffix, FloatSuffix::F);
    }

    #[test]
    fn decimal_2_0_with_l_suffix() {
        // `long double` value is stored as f64 — the task accepts the
        // lossy cast (see the doc comment on `decode_float`).
        let lit = fok("2.0L");
        assert_eq!(lit.value, 2.0);
        assert_eq!(lit.suffix, FloatSuffix::L);
    }

    #[test]
    fn decimal_simple_exponent_form() {
        // `1e5` has no `.` — this is the `digit-sequence exponent`
        // decimal-floating-constant form (C99 §6.4.4.2).
        let lit = fok("1e5");
        assert_eq!(lit.value, 1e5);
    }

    // ── decode_float: happy-path hex ────────────────────────────────

    #[test]
    fn hex_float_1_0_p0_equals_one() {
        let lit = fok("0x1.0p0");
        assert_eq!(lit.value, 1.0);
        assert_eq!(lit.suffix, FloatSuffix::None);
    }

    #[test]
    fn hex_float_1_8_p1_equals_three() {
        // 1.8 hex = 1 + 8/16 = 1.5; × 2^1 = 3.0.
        let lit = fok("0x1.8p1");
        assert_eq!(lit.value, 3.0);
    }

    #[test]
    fn hex_float_1_0_p3_equals_eight() {
        // Task acceptance bullet: `0x1.0p3` must evaluate to `8.0`.
        let lit = fok("0x1.0p3");
        assert_eq!(lit.value, 8.0);
    }

    #[test]
    fn hex_float_uppercase_prefix_and_p_exponent() {
        // `0X1.0P-1` — uppercase prefix, uppercase exponent, signed exp.
        let lit = fok("0X1.0P-1");
        assert_eq!(lit.value, 0.5);
    }

    #[test]
    fn hex_float_no_fraction_with_exponent_and_suffix() {
        // `0x1p3f` — no fraction, `f` suffix.
        let lit = fok("0x1p3f");
        assert_eq!(lit.value, 8.0);
        assert_eq!(lit.suffix, FloatSuffix::F);
    }

    #[test]
    fn hex_float_leading_dot_form() {
        // `0x.8p1` — no integer part, fraction only, × 2^1 = 1.0.
        let lit = fok("0x.8p1");
        assert_eq!(lit.value, 1.0);
    }

    // ── decode_float: overflow → +∞ ─────────────────────────────────

    #[test]
    fn decimal_overflow_returns_positive_infinity() {
        // Task acceptance bullet: overflow → +∞. The decoder returns
        // Ok(+∞) and the caller (phase7) is responsible for emitting
        // W0002 with the literal's span.
        let lit = fok("1e400");
        assert!(lit.value.is_infinite() && lit.value > 0.0, "got {}", lit.value);
    }

    #[test]
    fn hex_float_overflow_returns_positive_infinity() {
        // 2^20000 is far beyond f64::MAX; clamping + powi produces inf.
        let lit = fok("0x1p20000");
        assert!(lit.value.is_infinite() && lit.value > 0.0, "got {}", lit.value);
    }

    // ── decode_float: malformed ─────────────────────────────────────

    #[test]
    fn double_f_suffix_is_rejected() {
        // `1.0ff` — after stripping one `f`, the mantissa `1.0f` is
        // not a valid decimal float.
        let e = ferr("1.0ff");
        assert!(
            e.message.contains("malformed") || e.message.contains("floating"),
            "got: {}",
            e.message
        );
    }

    #[test]
    fn hex_float_without_exponent_is_rejected() {
        // Per C99 §6.4.4.2 the binary exponent is mandatory.
        let e = ferr("0x1.0");
        assert!(e.message.contains("exponent"), "got: {}", e.message);
    }

    #[test]
    fn hex_float_with_empty_exponent_is_rejected() {
        let e = ferr("0x1.0p");
        assert!(e.message.contains("exponent"), "got: {}", e.message);
    }

    #[test]
    fn hex_float_with_no_digits_is_rejected() {
        // `0x.p0` — both integer and fraction parts empty.
        let e = ferr("0x.p0");
        assert!(
            e.message.contains("no digits") || e.message.contains("exponent"),
            "got: {}",
            e.message
        );
    }

    #[test]
    fn hex_float_trailing_junk_is_rejected() {
        let e = ferr("0x1.0p0q");
        assert!(
            e.message.contains("unexpected") || e.message.contains("suffix"),
            "got: {}",
            e.message
        );
    }

    #[test]
    fn empty_float_is_rejected() {
        assert!(decode_float("").is_err());
    }

    #[test]
    fn bare_suffix_is_rejected() {
        // `f` alone — the suffix strip would leave nothing behind.
        let e = ferr("f");
        assert!(e.message.contains("no digits"), "got: {}", e.message);
    }
}
