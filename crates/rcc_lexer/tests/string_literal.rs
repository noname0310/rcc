//! String literals (task 03-lex/07).
//!
//! C99 §6.4.5 — string literals:
//!
//! ```text
//! string-literal := [prefix] " s-char-sequence_opt "
//! prefix         := L                          // wchar_t (C99)
//!                 | u | U | u8                 // C11, accepted for fwd-compat
//! s-char         := any member of the source character set except ",
//!                   \ or newline
//!                 | escape-sequence
//! escape-sequence := simple-escape | octal-escape | hex-escape | UCN
//! ```
//!
//! The lexer emits one `StringLit { enc }` pp-token per literal whose span
//! covers the whole thing including its prefix. Decoding to bytes and
//! adjacent-literal concatenation are deferred (phase 05).
//!
//! Diagnostics at this layer:
//! - E0008 for an unterminated literal (`"` without a closing `"` before
//!   a physical newline or EOF).
//! - E0007 for any unknown escape letter (reused from char constants).

use rcc_errors::codes::{E0007, E0008};
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

fn non_trivia(src: &str) -> Vec<rcc_lexer::PpToken> {
    tokenize(src)
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect()
}

fn only_string_lit(src: &str) -> rcc_lexer::PpToken {
    let toks = non_trivia(src);
    assert_eq!(toks.len(), 1, "src={src:?}: expected one non-ws token, got {toks:?}");
    let t = toks[0];
    assert!(
        matches!(t.kind, PpTokenKind::StringLit { .. }),
        "src={src:?}: expected StringLit, got {:?}",
        t.kind,
    );
    t
}

// ── Acceptance ───────────────────────────────────────────────────────

#[test]
fn acceptance_plain_narrow_string() {
    let src = r#""hello""#;
    let t = only_string_lit(src);
    assert_eq!(t.kind, PpTokenKind::StringLit { enc: StringEncoding::None });
    assert_eq!(t.span.lo.0, 0);
    assert_eq!(t.span.hi.0, src.len() as u32);
    assert!(diags(src).is_empty());
}

#[test]
fn acceptance_two_adjacent_string_lits_are_not_concatenated() {
    // From the task: `L"hi\\n" "bye"` must yield TWO StringLit tokens;
    // concatenation is a phase-05 (parser) job.
    let src = r#"L"hi\n" "bye""#;
    let toks = non_trivia(src);
    assert_eq!(toks.len(), 2, "expected two tokens, got {toks:?}");
    assert_eq!(toks[0].kind, PpTokenKind::StringLit { enc: StringEncoding::Wide });
    assert_eq!(toks[0].span.lo.0, 0);
    assert_eq!(toks[0].span.hi.0, 7);
    assert_eq!(toks[1].kind, PpTokenKind::StringLit { enc: StringEncoding::None });
    assert_eq!(toks[1].span.lo.0, 8);
    assert_eq!(toks[1].span.hi.0, src.len() as u32);
    assert!(diags(src).is_empty());
}

#[test]
fn acceptance_unterminated_at_eof_emits_e0008() {
    let src = r#""hi"#;
    let d = diags(src);
    assert_eq!(d.len(), 1, "expected exactly one diagnostic, got {d:?}");
    assert_eq!(d[0].code, Some(E0008));
    let toks = non_trivia(src);
    assert_eq!(toks.len(), 1);
    assert!(matches!(toks[0].kind, PpTokenKind::StringLit { enc: StringEncoding::None }));
    assert_eq!(toks[0].span.lo.0, 0);
    assert_eq!(toks[0].span.hi.0, src.len() as u32);
}

#[test]
fn unterminated_at_newline_emits_e0008_and_preserves_newline() {
    // Newline before closing `"` is unterminated per §6.4.5; the
    // newline itself must survive as a distinct `Newline` token so
    // directive boundaries are preserved.
    let src = "\"hi\nrest";
    let d = diags(src);
    assert!(d.iter().any(|d| d.code == Some(E0008)), "expected E0008, got {d:?}");
    let has_newline = tokenize(src).iter().any(|t| matches!(t.kind, PpTokenKind::Newline));
    assert!(has_newline, "newline token must survive past unterminated string literal");
}

// ── Encoding prefixes: L / u / U / u8 ───────────────────────────────

#[test]
#[allow(non_snake_case)]
fn encoding_prefix_L_wide() {
    let src = r#"L"x""#;
    let t = only_string_lit(src);
    assert_eq!(t.kind, PpTokenKind::StringLit { enc: StringEncoding::Wide });
    assert_eq!(t.span.lo.0, 0);
    assert_eq!(t.span.hi.0, src.len() as u32);
}

#[test]
fn encoding_prefix_u_utf16() {
    let src = r#"u"x""#;
    let t = only_string_lit(src);
    assert_eq!(t.kind, PpTokenKind::StringLit { enc: StringEncoding::Utf16 });
    assert_eq!(t.span.hi.0, src.len() as u32);
}

#[test]
#[allow(non_snake_case)]
fn encoding_prefix_U_utf32() {
    let src = r#"U"x""#;
    let t = only_string_lit(src);
    assert_eq!(t.kind, PpTokenKind::StringLit { enc: StringEncoding::Utf32 });
    assert_eq!(t.span.hi.0, src.len() as u32);
}

#[test]
fn encoding_prefix_u8_utf8() {
    let src = r#"u8"x""#;
    let t = only_string_lit(src);
    assert_eq!(t.kind, PpTokenKind::StringLit { enc: StringEncoding::Utf8 });
    assert_eq!(t.span.hi.0, src.len() as u32);
}

#[test]
fn empty_string_literal_is_well_formed() {
    let src = r#""""#;
    let t = only_string_lit(src);
    assert_eq!(t.kind, PpTokenKind::StringLit { enc: StringEncoding::None });
    assert_eq!(t.span.hi.0, 2);
    assert!(diags(src).is_empty());
}

// ── Prefix vs identifier / char-constant disambiguation ─────────────

#[test]
#[allow(non_snake_case)]
fn bare_L_without_quote_or_apostrophe_is_identifier() {
    let toks = non_trivia("L");
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].kind, PpTokenKind::Ident);
}

#[test]
#[allow(non_snake_case)]
fn L_followed_by_apostrophe_is_char_constant_not_string() {
    // Ensure the char-literal branch still wins for `L'`.
    let src = r"L'a'";
    let toks = non_trivia(src);
    assert_eq!(toks.len(), 1);
    assert!(matches!(toks[0].kind, PpTokenKind::CharConst { enc: StringEncoding::Wide }));
}

#[test]
fn u8_not_followed_by_quote_is_identifier() {
    let toks = non_trivia("u8foo");
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].kind, PpTokenKind::Ident);
    assert_eq!(toks[0].span.hi.0, 5);
}

#[test]
fn u_followed_by_non_quote_is_identifier() {
    let toks = non_trivia("u8");
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].kind, PpTokenKind::Ident);
}

#[test]
fn u8_followed_by_apostrophe_is_char_constant() {
    let src = "u8'x'";
    let toks = non_trivia(src);
    assert_eq!(toks.len(), 1);
    assert!(matches!(toks[0].kind, PpTokenKind::CharConst { enc: StringEncoding::Utf8 }));
}

// ── Escape-sequence coverage (mirrors char-literal suite) ───────────

#[test]
fn all_simple_escapes_are_recognised_in_string() {
    // C99 §6.4.5 shares the escape alphabet with §6.4.4.4.
    let escapes =
        [r"\'", r#"\""#, r"\?", r"\\", r"\a", r"\b", r"\e", r"\f", r"\n", r"\r", r"\t", r"\v"];
    for esc in escapes {
        let src = format!(r#""x{esc}y""#);
        let t = only_string_lit(&src);
        assert_eq!(t.span.hi.0, src.len() as u32, "esc={esc:?}");
        let d = diags(&src);
        assert!(d.is_empty(), "esc={esc:?} must not diagnose, got {d:?}");
    }
}

#[test]
fn hex_escape_consumes_all_hex_digits_in_string() {
    // Same maximal-munch as char literal: every trailing hex digit
    // folds into the `\x` run; overflow is a phase-05 concern.
    let src = r#""\xdeadbeef""#;
    let t = only_string_lit(src);
    assert_eq!(t.span.hi.0, src.len() as u32);
    assert!(diags(src).is_empty());
}

#[test]
fn octal_escape_up_to_three_digits_in_string() {
    let src = r#""\101""#;
    let t = only_string_lit(src);
    assert_eq!(t.span.hi.0, src.len() as u32);
    assert!(diags(src).is_empty());
}

#[test]
fn embedded_null_byte_via_octal_escape() {
    // `"\0"` — embedded NUL byte via the single-digit octal escape.
    // The lexer must treat `\0` as one escape char and stop scanning
    // the octal run because the next byte `"` is not octal.
    let src = r#""\0""#;
    let t = only_string_lit(src);
    assert_eq!(t.span.hi.0, src.len() as u32);
    assert!(diags(src).is_empty());
}

#[test]
fn ucn_short_and_long_form_in_string() {
    let src = r#""\u0041\U0001F600""#;
    let t = only_string_lit(src);
    assert_eq!(t.kind, PpTokenKind::StringLit { enc: StringEncoding::None });
    assert_eq!(t.span.hi.0, src.len() as u32);
    assert!(diags(src).is_empty());
}

#[test]
fn invalid_escape_letter_emits_e0007_in_string() {
    // Reuses the same escape scanner, so `\q` inside a string literal
    // emits E0007 without truncating the token.
    let src = r#""\q""#;
    let d = diags(src);
    assert!(d.iter().any(|d| d.code == Some(E0007)), "expected E0007, got {d:?}");
    let toks = non_trivia(src);
    assert_eq!(toks.len(), 1);
    assert!(matches!(toks[0].kind, PpTokenKind::StringLit { .. }));
    assert_eq!(toks[0].span.hi.0, src.len() as u32);
}

// ── UTF-8 continuation bytes (preserved verbatim) ───────────────────

#[test]
fn utf8_continuation_bytes_preserved_in_narrow_string() {
    // A narrow (unprefixed) string literal contains the UTF-8 bytes
    // for `κόσμε` (Greek "world"). The lexer must NOT decode them —
    // it only delimits the token and passes the raw bytes through.
    let src = "\"κόσμε\"";
    let t = only_string_lit(src);
    assert_eq!(t.kind, PpTokenKind::StringLit { enc: StringEncoding::None });
    // Span measured in source bytes (not characters).
    assert_eq!(t.span.hi.0, src.len() as u32);
    assert!(diags(src).is_empty());
}

// ── Line splicing inside string literals ────────────────────────────

#[test]
fn line_splicing_extends_string_body() {
    // `"\<LF>foo"` — the backslash-newline is eliminated in translation
    // phase 2 before the lexer ever sees it, so the literal lexes as a
    // single continuous string. The span is measured in physical bytes.
    let src = "\"\\\nfoo\"";
    let t = only_string_lit(src);
    assert_eq!(t.kind, PpTokenKind::StringLit { enc: StringEncoding::None });
    assert_eq!(t.span.lo.0, 0);
    assert_eq!(t.span.hi.0, src.len() as u32);
    assert!(diags(src).is_empty(), "line-spliced string must not diagnose: {:?}", diags(src));
}

// ── Follow-on tokens ────────────────────────────────────────────────

#[test]
fn string_lit_followed_by_identifier() {
    let src = r#""x" abc"#;
    let toks = non_trivia(src);
    assert_eq!(toks.len(), 2);
    assert!(matches!(toks[0].kind, PpTokenKind::StringLit { enc: StringEncoding::None }));
    assert_eq!(toks[0].span.hi.0, 3);
    assert_eq!(toks[1].kind, PpTokenKind::Ident);
    assert_eq!(toks[1].span.lo.0, 4);
    assert_eq!(toks[1].span.hi.0, 7);
}

#[test]
fn two_narrow_strings_separated_by_whitespace_emit_two_tokens() {
    // Sanity check: even without a prefix, adjacent string literals
    // MUST NOT merge at this layer.
    let src = r#""a" "b""#;
    let toks = non_trivia(src);
    assert_eq!(toks.len(), 2);
    for t in &toks {
        assert!(matches!(t.kind, PpTokenKind::StringLit { enc: StringEncoding::None }));
    }
}

// ── Consolidated `table()` per task 03-lex/10 ───────────────────────

#[test]
fn table() {
    // Positive: `(src, expected_encoding)` — `src` must lex to exactly
    // one `StringLit { enc }` whose slice re-spells the whole `src`
    // and whose encoding matches `enc`.
    let positive: &[(&str, StringEncoding)] = &[
        (r#""""#, StringEncoding::None),
        (r#""hello""#, StringEncoding::None),
        (r#""x\ty""#, StringEncoding::None),
        (r#""\n""#, StringEncoding::None),
        (r#""\xdeadbeef""#, StringEncoding::None),
        (r#""\101""#, StringEncoding::None),
        (r#""\u0041""#, StringEncoding::None),
        (r#""\U0001F600""#, StringEncoding::None),
        // Encoding prefixes (all four).
        (r#"L"x""#, StringEncoding::Wide),
        (r#"u"x""#, StringEncoding::Utf16),
        (r#"U"x""#, StringEncoding::Utf32),
        (r#"u8"x""#, StringEncoding::Utf8),
        // UTF-8 bytes are passed through verbatim (spelling = src).
        ("\"κόσμε\"", StringEncoding::None),
    ];

    for &(src, enc) in positive {
        let v = common::non_trivia(common::lex_all(src));
        assert_eq!(v.len(), 1, "positive src={src:?}: expected one non-ws token, got {v:?}");
        assert_eq!(
            v[0].0,
            PpTokenKind::StringLit { enc },
            "positive src={src:?}: expected StringLit {{ enc: {enc:?} }}, got {:?}",
            v[0].0,
        );
        assert_eq!(v[0].1, src, "positive src={src:?}: span slice must equal whole input");
        assert!(
            common::diag_codes(src).is_empty(),
            "positive src={src:?}: unexpected diagnostics {:?}",
            common::diag_codes(src),
        );
    }

    // Negative: each row must emit the named diagnostic code. The
    // token is still produced (recovery); we only care the diagnostic
    // surfaces so the preprocessor can see the error.
    let negative: &[(&str, &str)] = &[
        // Unterminated at EOF — no closing `"`.
        (r#""hi"#, E0008),
        // Unterminated at newline — literal `"` followed by `\n`.
        ("\"hi\nrest", E0008),
        // Unknown escape letter inside a well-terminated literal.
        (r#""\q""#, E0007),
        // Combined: unknown-escape on an unterminated literal still
        // yields both diagnostics — we only assert E0008 presence.
        ("\"\\q\n", E0008),
    ];

    for &(src, code) in negative {
        let codes = common::diag_codes(src);
        assert!(codes.contains(&code), "negative src={src:?}: expected {code}, got {codes:?}");
    }
}
