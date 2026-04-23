//! Identifier recognition + universal character names (task 03-lex/04).
//!
//! Covers:
//! - ASCII identifier fast path: `[_A-Za-z][_A-Za-z0-9]*`,
//! - `\uXXXX` / `\UXXXXXXXX` universal character names embedded in
//!   identifiers and starting identifiers,
//! - E0005 on malformed UCN (fewer than 4/8 hex digits) and on a
//!   disallowed UCN code point (C99 §6.4.3 constraint list + Annex D
//!   identifier-character rule from §6.4.2.1).

use rcc_errors::codes::E0005;
use rcc_lexer::{PpTokenKind, Tokenizer};
use rcc_session::Session;
use rcc_span::FileId;

mod common;

fn tokenize(src: &str) -> Vec<rcc_lexer::PpToken> {
    Tokenizer::new(FileId(0), src).collect()
}

/// Run `src` through the tokenizer with an attached capturing handler
/// and return the emitted diagnostics.
fn diags(src: &str) -> Vec<rcc_errors::Diagnostic> {
    let (mut sess, cap) = Session::for_test();
    let _: Vec<_> = Tokenizer::new(FileId(0), src).with_handler(&mut sess.handler).collect();
    cap.diagnostics()
}

// ── Well-formed identifiers (table-driven) ──────────────────────────

#[test]
fn table_driven_well_formed_identifiers() {
    // (src, expected identifier byte length)
    let cases: &[(&str, u32)] = &[
        // simple ASCII
        ("foo", 3),
        // single letter
        ("x", 1),
        // underscore-leading
        ("_under", 6),
        ("__x", 3),
        // numeric-embedded
        ("x42_7", 5),
        ("a1b2c3", 6),
        // UCN in middle
        (r"a\u00e9b", 8),
        // UCN at start, 4 hex digits (é)
        (r"\u00e9bauche", 12),
        // UCN at start, 8 hex digits (astral code point)
        (r"\U0001F600x", 11),
    ];

    for (src, expected_hi) in cases {
        let toks: Vec<_> = tokenize(src)
            .into_iter()
            .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
            .collect();

        assert!(!toks.is_empty(), "src={src:?} produced no tokens");
        let t = toks[0];
        assert_eq!(t.kind, PpTokenKind::Ident, "src={src:?} expected Ident, got {:?}", t.kind);
        assert_eq!(t.span.lo.0, 0, "src={src:?} lo mismatch");
        assert_eq!(t.span.hi.0, *expected_hi, "src={src:?} hi mismatch (tok span = {:?})", t.span);
    }
}

#[test]
fn well_formed_identifiers_emit_no_diagnostics() {
    for src in [r"foo", r"_under", r"x42_7", r"\u00e9bauche", r"\U0001F600x", r"a\u00e9b"] {
        let d = diags(src);
        assert!(d.is_empty(), "src={src:?} produced unexpected diagnostics: {d:?}");
    }
}

// ── Malformed UCNs ──────────────────────────────────────────────────

#[test]
fn malformed_short_ucn_emits_e0005() {
    // `\u12` — only 2 hex digits; 4 required.
    let src = r"\u12";
    let d = diags(src);
    assert_eq!(d.len(), 1, "expected exactly one diagnostic, got {d:?}");
    assert_eq!(d[0].code, Some(E0005));

    // Primary label must cover the bad escape bytes `\u12`.
    let primary = d[0].labels.iter().find(|l| l.primary).expect("primary label");
    assert_eq!(primary.span.lo.0, 0);
    assert_eq!(primary.span.hi.0, src.len() as u32);

    // Help must mention the exact bad escape bytes so the user can see
    // what we read.
    assert!(
        d[0].help.iter().any(|h| h.contains(r"\u12")),
        "expected help text to quote the bad escape bytes `\\u12`, got {:?}",
        d[0].help
    );
}

#[test]
fn malformed_long_ucn_emits_e0005() {
    // `\U0001F60` — only 7 hex digits; 8 required.
    let src = r"\U0001F60";
    let d = diags(src);
    assert!(d.iter().any(|d| d.code == Some(E0005)), "expected E0005, got {d:?}");
    let diag = d.iter().find(|d| d.code == Some(E0005)).unwrap();
    assert!(
        diag.help.iter().any(|h| h.contains(r"\U0001F60")),
        "help must quote the bad escape bytes, got {:?}",
        diag.help
    );
}

// ── Disallowed UCN code points ──────────────────────────────────────

#[test]
fn disallowed_short_ucn_dollar_emits_e0005() {
    // `\u0024` decodes to '$', which is *not* in C99 Annex D and so is
    // illegal as part of an identifier even though §6.4.3 exempts it
    // from the < 0x00A0 rule.
    let src = r"\u0024";
    let d = diags(src);
    assert!(d.iter().any(|d| d.code == Some(E0005)), "expected E0005, got {d:?}");
    let diag = d.iter().find(|d| d.code == Some(E0005)).unwrap();
    assert!(
        diag.help.iter().any(|h| h.contains(r"\u0024")),
        "help must quote the bad escape bytes, got {:?}",
        diag.help
    );
}

#[test]
fn disallowed_surrogate_ucn_emits_e0005() {
    // `\uD800` is a UTF-16 high-surrogate; §6.4.3 constraint list bans
    // D800..DFFF explicitly.
    let src = r"\uD800";
    let d = diags(src);
    assert!(d.iter().any(|d| d.code == Some(E0005)), "expected E0005, got {d:?}");
}

#[test]
fn disallowed_control_ucn_below_a0_emits_e0005() {
    // `\u007F` (DEL) is < 0xA0 and not one of the {0x24,0x40,0x60}
    // exceptions, so §6.4.3 constraint rejects it.
    let src = r"\u007F";
    let d = diags(src);
    assert!(d.iter().any(|d| d.code == Some(E0005)), "expected E0005, got {d:?}");
}

// ── Consolidated `table()` per task 03-lex/10 ───────────────────────
//
// ≥ 10 positive rows, each a `(src, expected_spelling)` pair whose lone
// non-whitespace token must be an `Ident` that re-spells `src`; ≥ 3
// negative rows, each a `(src, expected_code)` pair whose diagnostic
// stream must contain `expected_code`.

#[test]
fn table() {
    // Positive: each row lexes to exactly one `Ident` spanning the
    // whole source. The second column is the spelled slice we expect
    // back from `common::lex_all`; keeping it explicit makes the
    // table readable as a spec of the identifier grammar.
    let positive: &[(&str, &str)] = &[
        // ASCII-only forms.
        ("foo", "foo"),
        ("x", "x"),
        ("_under", "_under"),
        ("__x", "__x"),
        ("x42_7", "x42_7"),
        ("a1b2c3", "a1b2c3"),
        ("TheQuickBrownFox", "TheQuickBrownFox"),
        ("SCREAMING_SNAKE", "SCREAMING_SNAKE"),
        ("_0", "_0"),
        // UCN-bearing forms — the span covers the raw bytes including
        // the backslash-u escape.
        (r"a\u00e9b", r"a\u00e9b"),
        (r"\u00e9bauche", r"\u00e9bauche"),
        (r"\U0001F600x", r"\U0001F600x"),
    ];

    for &(src, expected) in positive {
        let v = common::non_trivia(common::lex_all(src));
        assert_eq!(v.len(), 1, "positive src={src:?}: expected one non-ws token, got {v:?}");
        assert_eq!(
            v[0].0,
            PpTokenKind::Ident,
            "positive src={src:?}: expected Ident, got {:?}",
            v[0].0
        );
        assert_eq!(v[0].1, expected, "positive src={src:?}: span slice mismatch");
        assert!(
            common::diag_codes(src).is_empty(),
            "positive src={src:?}: unexpected diagnostics {:?}",
            common::diag_codes(src),
        );
    }

    // Negative: each row must trigger the named diagnostic code at
    // least once. The token is still produced so lex recovery stays
    // forward-moving.
    let negative: &[(&str, &str)] = &[
        // Malformed short UCN (2 hex digits instead of 4).
        (r"\u12", E0005),
        // Malformed long UCN (7 hex digits instead of 8).
        (r"\U0001F60", E0005),
        // `\u0024` → `$` is not in Annex D.
        (r"\u0024", E0005),
        // UTF-16 high-surrogate is banned by §6.4.3 constraint list.
        (r"\uD800", E0005),
        // `\u007F` (DEL) is < 0xA0 with no §6.4.3 exception.
        (r"\u007F", E0005),
    ];

    for &(src, code) in negative {
        let codes = common::diag_codes(src);
        assert!(codes.contains(&code), "negative src={src:?}: expected {code}, got {codes:?}");
    }
}
