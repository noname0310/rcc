//! Character constants (task 03-lex/06).
//!
//! C99 §6.4.4.4 — character constants:
//!
//! ```text
//! character-constant := [prefix] ' c-char-sequence '
//! prefix             := L                          // wchar_t (C99)
//!                     | u | U | u8                 // C11, accepted for fwd-compat
//! c-char             := any member of the source character set except ',
//!                       \ or newline
//!                     | escape-sequence
//! escape-sequence    := simple-escape | octal-escape | hex-escape | UCN
//! simple-escape      := \' \" \? \\ \a \b \e \f \n \r \t \v
//! octal-escape       := \ octal-digit{1,3}
//! hex-escape         := \x hex-digit+
//! ```
//!
//! The lexer produces a single `CharConst { enc }` pp-token whose span
//! covers the entire literal including its prefix. Byte-value decoding
//! is deferred to phase 05.

use rcc_errors::codes::{E0006, E0007};
use rcc_lexer::{PpTokenKind, StringEncoding, Tokenizer};
use rcc_session::Session;
use rcc_span::FileId;

mod common;

fn tokenize(src: &str) -> Vec<rcc_lexer::PpToken> {
    Tokenizer::new(FileId(0), src).collect()
}

fn diags(src: &str) -> Vec<rcc_errors::Diagnostic> {
    let (mut sess, cap) = Session::for_test();
    let _: Vec<_> = Tokenizer::new(FileId(0), src).with_handler(&mut sess.handler).collect();
    cap.diagnostics()
}

fn only_char_const(src: &str) -> rcc_lexer::PpToken {
    let toks: Vec<_> = tokenize(src)
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 1, "src={src:?}: expected one non-ws token, got {toks:?}");
    let t = toks[0];
    assert!(
        matches!(t.kind, PpTokenKind::CharConst { .. }),
        "src={src:?}: expected CharConst, got {:?}",
        t.kind,
    );
    t
}

// ── Acceptance: all four task examples classify correctly ───────────

#[test]
fn acceptance_plain_narrow_char_const() {
    let src = "'a'";
    let t = only_char_const(src);
    assert_eq!(t.kind, PpTokenKind::CharConst { enc: StringEncoding::None });
    assert_eq!(t.span.lo.0, 0);
    assert_eq!(t.span.hi.0, src.len() as u32);
    assert!(diags(src).is_empty(), "well-formed constant must not diagnose");
}

#[test]
fn acceptance_wide_hex_escape() {
    let src = r"L'\xff'";
    let t = only_char_const(src);
    assert_eq!(t.kind, PpTokenKind::CharConst { enc: StringEncoding::Wide });
    assert_eq!(t.span.lo.0, 0);
    assert_eq!(t.span.hi.0, src.len() as u32);
    assert!(diags(src).is_empty());
}

#[test]
fn acceptance_escaped_backslash() {
    // C source `'\\'` — a character constant whose body is a single
    // escaped backslash. In Rust source we must double-escape both
    // the backslash and the quote.
    let src = "'\\\\'";
    assert_eq!(src, r"'\\'");
    let t = only_char_const(src);
    assert_eq!(t.kind, PpTokenKind::CharConst { enc: StringEncoding::None });
    assert_eq!(t.span.lo.0, 0);
    assert_eq!(t.span.hi.0, src.len() as u32);
    assert!(diags(src).is_empty());
}

#[test]
fn acceptance_ucn_short_form() {
    let src = r"'\u0041'";
    let t = only_char_const(src);
    assert_eq!(t.kind, PpTokenKind::CharConst { enc: StringEncoding::None });
    assert_eq!(t.span.lo.0, 0);
    assert_eq!(t.span.hi.0, src.len() as u32);
    assert!(
        diags(src).is_empty(),
        "well-formed UCN must not diagnose at lex time: {:?}",
        diags(src)
    );
}

// ── Acceptance: unterminated at EOF emits E0006 ─────────────────────

#[test]
fn acceptance_unterminated_at_eof_emits_e0006() {
    // `'a` — quote opened, no closing quote before EOF.
    let src = "'a";
    let d = diags(src);
    assert_eq!(d.len(), 1, "expected exactly one diagnostic, got {d:?}");
    assert_eq!(d[0].code, Some(E0006));
    // Token is still emitted so callers can continue lexing.
    let toks: Vec<_> = tokenize(src)
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 1);
    assert!(matches!(toks[0].kind, PpTokenKind::CharConst { enc: StringEncoding::None }));
    assert_eq!(toks[0].span.lo.0, 0);
    assert_eq!(toks[0].span.hi.0, src.len() as u32);
}

#[test]
fn unterminated_at_newline_emits_e0006() {
    // `'a\n` — newline before closing quote is also unterminated; the
    // token must NOT swallow the newline (the newline survives as a
    // separate `Newline` token so directive boundaries are preserved).
    let src = "'a\nb";
    let d = diags(src);
    assert!(d.iter().any(|d| d.code == Some(E0006)), "expected E0006, got {d:?}");
    let newline_present = tokenize(src).iter().any(|t| matches!(t.kind, PpTokenKind::Newline));
    assert!(newline_present, "newline token must survive past unterminated char const");
}

// ── Encoding prefixes: L / u / U / u8 ───────────────────────────────

#[test]
#[allow(non_snake_case)]
fn encoding_prefix_L_wide() {
    let t = only_char_const("L'x'");
    assert_eq!(t.kind, PpTokenKind::CharConst { enc: StringEncoding::Wide });
    assert_eq!(t.span.lo.0, 0);
    assert_eq!(t.span.hi.0, 4);
}

#[test]
fn encoding_prefix_u_utf16() {
    let t = only_char_const("u'x'");
    assert_eq!(t.kind, PpTokenKind::CharConst { enc: StringEncoding::Utf16 });
    assert_eq!(t.span.lo.0, 0);
    assert_eq!(t.span.hi.0, 4);
}

#[test]
#[allow(non_snake_case)]
fn encoding_prefix_U_utf32() {
    let t = only_char_const("U'x'");
    assert_eq!(t.kind, PpTokenKind::CharConst { enc: StringEncoding::Utf32 });
    assert_eq!(t.span.lo.0, 0);
    assert_eq!(t.span.hi.0, 4);
}

#[test]
fn encoding_prefix_u8_utf8() {
    let t = only_char_const("u8'x'");
    assert_eq!(t.kind, PpTokenKind::CharConst { enc: StringEncoding::Utf8 });
    assert_eq!(t.span.lo.0, 0);
    assert_eq!(t.span.hi.0, 5);
}

// ── Prefix vs identifier disambiguation ─────────────────────────────

#[test]
#[allow(non_snake_case)]
fn bare_L_without_quote_is_identifier() {
    // `L` alone must lex as an identifier, not a char-constant start.
    let toks: Vec<_> = tokenize("L")
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].kind, PpTokenKind::Ident);
}

#[test]
fn u8_not_followed_by_quote_is_identifier() {
    // `u8foo` is an identifier; the `u8` prefix only triggers when the
    // very next character is `'`.
    let toks: Vec<_> = tokenize("u8foo")
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].kind, PpTokenKind::Ident);
    assert_eq!(toks[0].span.hi.0, 5);
}

#[test]
fn u_followed_by_non_quote_is_identifier() {
    // `u8` (bare, followed by EOF) must be an identifier — no char
    // constant possible without the trailing `'`.
    let toks: Vec<_> = tokenize("u8")
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].kind, PpTokenKind::Ident);
}

// ── Multi-character constants are implementation-defined, not errors ─

#[test]
fn multi_char_constant_produces_no_diagnostic() {
    // `'ab'` is legal per §6.4.4.4p10 — implementation-defined value,
    // but the lexer must keep the bytes and NOT error.
    let src = "'ab'";
    let t = only_char_const(src);
    assert_eq!(t.kind, PpTokenKind::CharConst { enc: StringEncoding::None });
    assert_eq!(t.span.hi.0, src.len() as u32);
    assert!(diags(src).is_empty(), "multi-char constants must not diagnose: {:?}", diags(src));
}

// ── Escape-sequence coverage ─────────────────────────────────────────

#[test]
fn all_simple_escapes_are_recognised() {
    // GNU hosted Linux headers can contain `\e`; accept it as ESC.
    let escapes =
        [r"\'", r#"\""#, r"\?", r"\\", r"\a", r"\b", r"\e", r"\f", r"\n", r"\r", r"\t", r"\v"];
    for esc in escapes {
        let src = format!("'{esc}'");
        let t = only_char_const(&src);
        assert_eq!(t.span.hi.0, src.len() as u32, "esc={esc:?}");
        let d = diags(&src);
        assert!(d.is_empty(), "esc={esc:?} must not diagnose, got {d:?}");
    }
}

#[test]
fn octal_escape_up_to_three_digits() {
    // `'\101'` — three octal digits; no error.
    let src = r"'\101'";
    let t = only_char_const(src);
    assert_eq!(t.span.hi.0, src.len() as u32);
    assert!(diags(src).is_empty());

    // `'\0'` — single octal digit.
    let src = r"'\0'";
    let t = only_char_const(src);
    assert_eq!(t.span.hi.0, src.len() as u32);
    assert!(diags(src).is_empty());

    // `'\7'` — single octal digit, boundary.
    let src = r"'\7'";
    let _ = only_char_const(src);
    assert!(diags(src).is_empty());
}

#[test]
fn octal_escape_stops_at_non_octal() {
    // `'\19'` — `\1` is the octal escape, `9` is a literal `9` c-char
    // (maximal munch of octal is 3 digits, but `9` is not octal so
    // the run stops at `\1`). Total span is 5 bytes: `'`, `\`, `1`,
    // `9`, `'`. No diagnostic.
    let src = r"'\19'";
    let t = only_char_const(src);
    assert_eq!(t.span.hi.0, src.len() as u32);
    assert!(diags(src).is_empty(), "got: {:?}", diags(src));
}

#[test]
fn hex_escape_consumes_all_hex_digits() {
    // `'\xdeadbeef'` — lexer swallows all hex digits; overflow/
    // truncation is a phase-05 concern.
    let src = r"'\xdeadbeef'";
    let t = only_char_const(src);
    assert_eq!(t.span.hi.0, src.len() as u32);
    assert!(diags(src).is_empty());
}

#[test]
fn ucn_long_form_in_char_const() {
    // `\U0001F600` — 8 hex digit UCN (astral code point).
    let src = r"'\U0001F600'";
    let t = only_char_const(src);
    assert_eq!(t.kind, PpTokenKind::CharConst { enc: StringEncoding::None });
    assert_eq!(t.span.hi.0, src.len() as u32);
    assert!(diags(src).is_empty());
}

// ── Invalid escape → E0007 ───────────────────────────────────────────

#[test]
fn invalid_escape_letter_emits_e0007() {
    // `'\q'` — `\q` is not a C99 escape sequence.
    let src = r"'\q'";
    let d = diags(src);
    assert!(d.iter().any(|d| d.code == Some(E0007)), "expected E0007, got {d:?}");
    // The token is still emitted so lexing can continue.
    let toks: Vec<_> = tokenize(src)
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 1);
    assert!(matches!(toks[0].kind, PpTokenKind::CharConst { .. }));
    assert_eq!(toks[0].span.hi.0, src.len() as u32);
}

// ── Sanity: a subsequent token after a char constant still lexes ─────

#[test]
fn char_const_followed_by_identifier() {
    let src = "'x' abc";
    let toks: Vec<_> = tokenize(src)
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 2);
    assert!(matches!(toks[0].kind, PpTokenKind::CharConst { enc: StringEncoding::None }));
    assert_eq!(toks[0].span.hi.0, 3);
    assert_eq!(toks[1].kind, PpTokenKind::Ident);
    assert_eq!(toks[1].span.lo.0, 4);
    assert_eq!(toks[1].span.hi.0, 7);
}

// ── Consolidated `table()` per task 03-lex/10 ───────────────────────

#[test]
fn table() {
    // Positive: `(src, expected_encoding)` — `src` must lex to exactly
    // one `CharConst { enc }` spelling the whole `src`.
    let positive: &[(&str, StringEncoding)] = &[
        ("'a'", StringEncoding::None),
        ("'ab'", StringEncoding::None), // multi-char, impl-defined but legal.
        (r"'\''", StringEncoding::None),
        (r"'\\'", StringEncoding::None),
        (r"'\n'", StringEncoding::None),
        (r"'\xff'", StringEncoding::None),
        (r"'\101'", StringEncoding::None),
        (r"'\0'", StringEncoding::None),
        (r"'\u0041'", StringEncoding::None),
        (r"'\U0001F600'", StringEncoding::None),
        ("L'x'", StringEncoding::Wide),
        ("u'x'", StringEncoding::Utf16),
        ("U'x'", StringEncoding::Utf32),
        ("u8'x'", StringEncoding::Utf8),
    ];

    for &(src, enc) in positive {
        let v = common::non_trivia(common::lex_all(src));
        assert_eq!(v.len(), 1, "positive src={src:?}: expected one non-ws token, got {v:?}");
        assert_eq!(
            v[0].0,
            PpTokenKind::CharConst { enc },
            "positive src={src:?}: expected CharConst {{ enc: {enc:?} }}, got {:?}",
            v[0].0,
        );
        assert_eq!(v[0].1, src, "positive src={src:?}: span slice must equal whole input");
        assert!(
            common::diag_codes(src).is_empty(),
            "positive src={src:?}: unexpected diagnostics {:?}",
            common::diag_codes(src),
        );
    }

    // Negative: each row must emit the named diagnostic code.
    let negative: &[(&str, &str)] = &[
        // Unterminated at EOF — no closing `'`.
        ("'a", E0006),
        // Unterminated at newline — closing `'` missing before `\n`.
        ("'a\nrest", E0006),
        // Unknown escape letter inside a well-terminated constant.
        (r"'\q'", E0007),
        // Wide constant unterminated at EOF — prefix-bearing variant.
        ("L'x", E0006),
    ];

    for &(src, code) in negative {
        let codes = common::diag_codes(src);
        assert!(codes.contains(&code), "negative src={src:?}: expected {code}, got {codes:?}");
    }
}
