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

use crate::token::{IntLiteral, IntSuffix};

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
}
