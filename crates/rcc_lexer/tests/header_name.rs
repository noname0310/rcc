//! Header-name context (task 03-lex/09).
//!
//! C99 §6.4p4 / §6.4.7: `header-name` pp-tokens are recognised only
//! inside `#include` directives (and implementation-defined spots of
//! `#pragma`). The default tokenizer loop must therefore *never*
//! spontaneously produce `HeaderName`; the preprocessor calls the
//! one-shot [`Tokenizer::lex_header_name`] API after it has seen
//! `# include`.
//!
//! Covered here:
//! - default `a < b` still tokenises as `Ident` / `Lt` / `Ident`,
//! - explicit `lex_header_name` on `<stdio.h>` yields one `HeaderName`,
//! - same for `"stdio.h"`,
//! - unterminated `<foo` (newline or EOF) → E0010,
//! - unterminated `"foo` (newline or EOF) → E0010,
//! - returns `None` when the next non-whitespace char is neither
//!   `<` nor `"`.

use rcc_errors::codes::E0010;
use rcc_lexer::{PpTokenKind, Punct, Tokenizer};
use rcc_session::Session;
use rcc_span::FileId;

// ── Default loop never produces HeaderName ──────────────────────────

#[test]
fn default_tokenisation_of_angle_expression_yields_lt_not_header_name() {
    // `a < b`: in expression context this is Ident / Lt / Ident. The
    // plain `next()` loop has no way to know it is inside `#include`,
    // so it must prefer the punctuator interpretation.
    let toks: Vec<_> = Tokenizer::new(FileId(0), "a < b")
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();

    let kinds: Vec<_> = toks.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![PpTokenKind::Ident, PpTokenKind::Punct(Punct::Lt), PpTokenKind::Ident],
        "default loop on `a < b` must not emit HeaderName",
    );
}

#[test]
fn default_loop_on_angle_path_still_breaks_into_punctuators() {
    // Without `lex_header_name`, `<stdio.h>` must fall back to the
    // ordinary max-munch punctuator rules — no HeaderName anywhere.
    let toks: Vec<_> = Tokenizer::new(FileId(0), "<stdio.h>").collect();
    for t in &toks {
        assert_ne!(
            t.kind,
            PpTokenKind::HeaderName,
            "default next() must never produce HeaderName, got {toks:?}",
        );
    }
}

#[test]
fn default_loop_on_quoted_path_sees_string_literal_not_header_name() {
    // Outside `#include` a double-quoted run is an ordinary string
    // literal. That's fine; we only care that it is NOT HeaderName.
    let toks: Vec<_> = Tokenizer::new(FileId(0), "\"stdio.h\"").collect();
    for t in &toks {
        assert_ne!(
            t.kind,
            PpTokenKind::HeaderName,
            "default next() must never produce HeaderName, got {toks:?}",
        );
    }
}

// ── Explicit lex_header_name: angle form ────────────────────────────

#[test]
fn explicit_angle_header_name_yields_one_token() {
    let src = "<stdio.h>";
    let mut tok = Tokenizer::new(FileId(0), src);
    let hn = tok.lex_header_name().expect("expected a HeaderName token");
    assert_eq!(hn.kind, PpTokenKind::HeaderName);
    assert_eq!(hn.span.lo.0, 0);
    assert_eq!(hn.span.hi.0, src.len() as u32);

    // Cursor must be at EOF after the closing `>`.
    assert!(tok.next().is_none(), "cursor should be at EOF after the header-name");
}

#[test]
fn explicit_angle_header_name_after_whitespace() {
    // The preprocessor will usually have consumed the `include` ident
    // via `next()`, leaving horizontal whitespace before the `<`.
    // `lex_header_name` must absorb that whitespace.
    let src = "   <stdio.h>\n";
    let mut tok = Tokenizer::new(FileId(0), src);
    let hn = tok.lex_header_name().expect("expected a HeaderName token");
    assert_eq!(hn.kind, PpTokenKind::HeaderName);
    // The span covers exactly `<stdio.h>`, not the leading whitespace.
    let lt = src.find('<').unwrap() as u32;
    let gt = src.find('>').unwrap() as u32 + 1;
    assert_eq!(hn.span.lo.0, lt);
    assert_eq!(hn.span.hi.0, gt);

    // The newline survives untouched in the outer stream.
    let after = tok.next().expect("Newline after header-name");
    assert_eq!(after.kind, PpTokenKind::Newline);
}

// ── Explicit lex_header_name: quoted form ───────────────────────────

#[test]
fn explicit_quoted_header_name_yields_one_token() {
    let src = "\"stdio.h\"";
    let mut tok = Tokenizer::new(FileId(0), src);
    let hn = tok.lex_header_name().expect("expected a HeaderName token");
    assert_eq!(hn.kind, PpTokenKind::HeaderName);
    assert_eq!(hn.span.lo.0, 0);
    assert_eq!(hn.span.hi.0, src.len() as u32);
    assert!(tok.next().is_none());
}

#[test]
fn explicit_quoted_header_name_does_not_apply_string_escape_rules() {
    // `"foo\"bar"` as a *string literal* is a well-formed escape of
    // `"`; as a *header-name* the backslash is literal and the first
    // `"` after `foo\` closes the header-name. Verify we use the
    // header-name rule — no escape processing — so the token ends at
    // the first unescaped `"`.
    let src = "\"foo\\\"bar\"";
    //         0 1 2 3 4 5 6 7 8 9  — byte positions
    //         "   f   o   o   \   "   b   a   r   "
    let mut tok = Tokenizer::new(FileId(0), src);
    let hn = tok.lex_header_name().expect("expected a HeaderName token");
    assert_eq!(hn.kind, PpTokenKind::HeaderName);
    // The first unescaped `"` is at index 5, and the HeaderName span
    // covers bytes 0..=5.
    assert_eq!(hn.span.lo.0, 0);
    assert_eq!(hn.span.hi.0, 6);
}

// ── Unterminated → E0010 ────────────────────────────────────────────

fn run_with_handler(
    src: &str,
    action: impl for<'a> FnOnce(&mut Tokenizer<'a>),
) -> Vec<rcc_errors::Diagnostic> {
    let (mut sess, cap) = Session::for_test();
    {
        let mut tok = Tokenizer::new(FileId(0), src).with_handler(&mut sess.handler);
        action(&mut tok);
    }
    cap.diagnostics()
}

#[test]
fn angle_header_name_unterminated_by_eof_emits_e0010() {
    let src = "<foo";
    let diags = run_with_handler(src, |tok| {
        let hn = tok.lex_header_name().expect("token still produced for recovery");
        assert_eq!(hn.kind, PpTokenKind::HeaderName);
        // The recovery token spans everything consumed so far.
        assert_eq!(hn.span.lo.0, 0);
        assert_eq!(hn.span.hi.0, src.len() as u32);
    });
    assert_eq!(diags.len(), 1, "expected exactly one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, Some(E0010));
}

#[test]
fn angle_header_name_unterminated_by_newline_emits_e0010() {
    let src = "<foo\nrest";
    let diags = run_with_handler(src, |tok| {
        let hn = tok.lex_header_name().expect("token still produced for recovery");
        assert_eq!(hn.kind, PpTokenKind::HeaderName);
        // The newline itself is NOT consumed — directive boundaries
        // must survive so the preprocessor can resynchronise.
        let nl = src.find('\n').unwrap() as u32;
        assert_eq!(hn.span.lo.0, 0);
        assert_eq!(hn.span.hi.0, nl);
        // Next token from the outer loop must be the preserved Newline.
        let nxt = tok.next().expect("Newline after unterminated header-name");
        assert_eq!(nxt.kind, PpTokenKind::Newline);
    });
    assert!(diags.iter().any(|d| d.code == Some(E0010)), "expected E0010 in {diags:?}");
}

#[test]
fn quoted_header_name_unterminated_by_eof_emits_e0010() {
    let src = "\"foo";
    let diags = run_with_handler(src, |tok| {
        let hn = tok.lex_header_name().expect("token still produced for recovery");
        assert_eq!(hn.kind, PpTokenKind::HeaderName);
        assert_eq!(hn.span.lo.0, 0);
        assert_eq!(hn.span.hi.0, src.len() as u32);
    });
    assert_eq!(diags.len(), 1, "expected exactly one diagnostic, got {diags:?}");
    assert_eq!(diags[0].code, Some(E0010));
}

#[test]
fn quoted_header_name_unterminated_by_newline_emits_e0010() {
    let src = "\"foo\nrest";
    let diags = run_with_handler(src, |tok| {
        let hn = tok.lex_header_name().expect("token still produced for recovery");
        assert_eq!(hn.kind, PpTokenKind::HeaderName);
        let nl = src.find('\n').unwrap() as u32;
        assert_eq!(hn.span.hi.0, nl, "must stop before the newline");
    });
    assert!(diags.iter().any(|d| d.code == Some(E0010)), "expected E0010 in {diags:?}");
}

// ── No header-name when the next char is something else ─────────────

#[test]
fn lex_header_name_returns_none_on_non_header_start() {
    // After `#include 123`, `lex_header_name` cannot recognise a
    // header-name — it must return None without consuming the digit.
    let mut tok = Tokenizer::new(FileId(0), "123");
    assert!(tok.lex_header_name().is_none());
    // The `123` must still be lexable by the ordinary loop.
    let nxt = tok.next().expect("pp-number still reachable");
    assert!(matches!(nxt.kind, PpTokenKind::PpNumber(_)));
    assert_eq!(nxt.span.lo.0, 0);
}

#[test]
fn lex_header_name_returns_none_on_eof() {
    let mut tok = Tokenizer::new(FileId(0), "   ");
    assert!(tok.lex_header_name().is_none());
    assert!(tok.next().is_none());
}
