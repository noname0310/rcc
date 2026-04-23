//! Whitespace, newline, and comment handling (task 03-lex/03).
//!
//! Covers:
//! - horizontal whitespace run collapsing (and `preserve_whitespace` mode),
//! - `PpTokenKind::Newline` per physical newline (directive boundary marker),
//! - `//` line comments eaten up to (but not including) the EOL,
//! - `/* ... */` block comments reduced to a single space,
//! - nested `/*` → E0003,
//! - unterminated block comment → E0004.

use rcc_errors::codes::{E0003, E0004};
use rcc_lexer::{PpTokenKind, Tokenizer};
use rcc_session::Session;
use rcc_span::FileId;

/// Collect all tokens produced by the (default-configured) tokenizer.
fn tokenize(src: &str) -> Vec<rcc_lexer::PpToken> {
    Tokenizer::new(FileId(0), src).collect()
}

// ── Line comments and newlines ──────────────────────────────────────

#[test]
fn line_comment_is_skipped_up_to_newline() {
    let src = "// comment\nfoo";
    let tokens = tokenize(src);

    // The `//` run is skipped (default preserve_whitespace = false).
    // Remaining tokens: Newline, then whatever recognises `foo`.
    assert!(!tokens.is_empty(), "expected at least a Newline token");
    assert_eq!(tokens[0].kind, PpTokenKind::Newline, "first post-comment token must be Newline");

    // The newline itself lives at the position of `\n`.
    let nl_pos = src.find('\n').unwrap() as u32;
    assert_eq!(tokens[0].span.lo.0, nl_pos);
    assert_eq!(tokens[0].span.hi.0, nl_pos + 1);

    // Every post-newline token must lie inside `foo`.
    let foo_start = nl_pos + 1;
    let foo_end = src.len() as u32;
    for t in &tokens[1..] {
        assert!(
            t.span.lo.0 >= foo_start && t.span.hi.0 <= foo_end,
            "post-newline token {t:?} must lie within `foo`"
        );
    }
}

#[test]
fn line_comment_does_not_eat_newline() {
    // `//xyz` then a newline then nothing: we must still see a Newline.
    let src = "//xyz\n";
    let tokens = tokenize(src);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, PpTokenKind::Newline);
}

#[test]
fn line_comment_at_eof_is_ok() {
    // `//xyz` with no trailing newline should just disappear.
    let src = "//xyz";
    let tokens = tokenize(src);
    assert!(tokens.is_empty(), "expected no tokens, got {tokens:?}");
}

// ── Block comments ──────────────────────────────────────────────────

#[test]
fn block_comment_reduces_to_single_space() {
    // `/* a */ b /* c */`: both block comments are skipped entirely;
    // only the middle `b` survives. The surviving token must span
    // exactly the byte range of `b`.
    let src = "/* a */ b /* c */";
    let tokens = tokenize(src);

    let b_pos = src.find('b').unwrap() as u32;
    // Filter out any incidental Whitespace/Newline (none expected, but
    // be defensive).
    let non_ws: Vec<_> = tokens
        .iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();

    assert_eq!(non_ws.len(), 1, "expected 1 non-ws token, got {non_ws:?}");
    assert_eq!(non_ws[0].span.lo.0, b_pos);
    assert_eq!(non_ws[0].span.hi.0, b_pos + 1);
}

#[test]
fn block_comment_spans_newlines_without_emitting_them() {
    // A block comment that contains `\n` inside: per C99 §5.1.1.2 phase 3,
    // the whole comment reduces to one space; no Newline tokens are
    // emitted for comment-internal newlines (they must not terminate a
    // preprocessor directive).
    let src = "/* a\nb */x";
    let tokens = tokenize(src);

    let non_ws: Vec<_> =
        tokens.iter().filter(|t| !matches!(t.kind, PpTokenKind::Whitespace)).collect();

    // Only `x` survives; no Newline at all.
    assert!(
        non_ws.iter().all(|t| t.kind != PpTokenKind::Newline),
        "block comment must not emit Newline: {non_ws:?}"
    );
    let x_pos = src.find('x').unwrap() as u32;
    assert!(
        non_ws.iter().any(|t| t.span.lo.0 == x_pos && t.span.hi.0 == x_pos + 1),
        "expected a token covering `x`, got {non_ws:?}"
    );
}

#[test]
fn unterminated_block_comment_emits_e0004_with_label_at_opening() {
    let src = "/* never closed";
    let (mut sess, cap) = Session::for_test();

    let _tokens: Vec<_> = Tokenizer::new(FileId(0), src).with_handler(&mut sess.handler).collect();

    let diags = cap.diagnostics();
    assert_eq!(diags.len(), 1, "expected exactly one diagnostic, got {diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, Some(E0004));

    // Primary label must point at the opening `/*` (2 bytes at offset 0).
    let primary = d.labels.iter().find(|l| l.primary).expect("primary label");
    assert_eq!(primary.span.lo.0, 0);
    assert_eq!(primary.span.hi.0, 2, "primary label should cover just `/*`");
}

#[test]
fn nested_block_comment_emits_e0003() {
    // Outer comment contains another `/*` → C has no nested comments.
    let src = "/* outer /* inner */ tail";
    let (mut sess, cap) = Session::for_test();

    let _tokens: Vec<_> = Tokenizer::new(FileId(0), src).with_handler(&mut sess.handler).collect();

    let diags = cap.diagnostics();
    assert!(diags.iter().any(|d| d.code == Some(E0003)), "expected E0003, got {diags:?}");

    // E0003 primary label must point at the nested `/*` occurrence.
    let d = diags.iter().find(|d| d.code == Some(E0003)).unwrap();
    let primary = d.labels.iter().find(|l| l.primary).expect("primary label");
    let nested_pos = src.find("/* inner").unwrap() as u32;
    assert_eq!(primary.span.lo.0, nested_pos);
    assert_eq!(primary.span.hi.0, nested_pos + 2, "label should cover just the nested `/*`");
}

#[test]
fn nested_block_comment_reports_once_per_comment() {
    // Three nested `/*` occurrences inside one outer block: we still
    // only emit one E0003 so the user doesn't get a diagnostic storm.
    let src = "/* a /* b /* c /* d */ end";
    let (mut sess, cap) = Session::for_test();

    let _tokens: Vec<_> = Tokenizer::new(FileId(0), src).with_handler(&mut sess.handler).collect();

    let diags = cap.diagnostics();
    let e0003_count = diags.iter().filter(|d| d.code == Some(E0003)).count();
    assert_eq!(e0003_count, 1, "expected exactly one E0003, got {diags:?}");
}

// ── Horizontal whitespace ───────────────────────────────────────────

#[test]
fn default_mode_collapses_horizontal_whitespace() {
    // `a  b`: the two-space run is dropped completely in default mode.
    let src = "a  b";
    let tokens = tokenize(src);

    // No Whitespace tokens should appear in default mode.
    assert!(
        tokens.iter().all(|t| t.kind != PpTokenKind::Whitespace),
        "default mode must not emit Whitespace: {tokens:?}"
    );
}

#[test]
fn preserve_whitespace_mode_keeps_one_run_per_span() {
    // `a  b`: preserve mode emits exactly one Whitespace token spanning
    // both spaces, flanked by the two identifier-looking tokens.
    let src = "a  b";
    let tokens: Vec<_> = Tokenizer::new(FileId(0), src).preserve_whitespace(true).collect();

    let ws: Vec<_> =
        tokens.iter().enumerate().filter(|(_, t)| t.kind == PpTokenKind::Whitespace).collect();
    assert_eq!(ws.len(), 1, "expected exactly one Whitespace token, got {tokens:?}");
    let (_, ws_tok) = ws[0];
    assert_eq!(ws_tok.span.lo.0, 1, "whitespace run starts right after `a`");
    assert_eq!(ws_tok.span.hi.0, 3, "whitespace run ends right before `b`");
}

#[test]
fn preserve_whitespace_mode_emits_whitespace_for_block_comment() {
    // Even a comment reduces to whitespace in preserve mode.
    let src = "a/* c */b";
    let tokens: Vec<_> = Tokenizer::new(FileId(0), src).preserve_whitespace(true).collect();

    let ws_count = tokens.iter().filter(|t| t.kind == PpTokenKind::Whitespace).count();
    assert_eq!(ws_count, 1, "expected one whitespace token from the comment, got {tokens:?}");
    let ws = tokens.iter().find(|t| t.kind == PpTokenKind::Whitespace).unwrap();
    assert_eq!(ws.span.lo.0, 1, "comment-whitespace lo");
    assert_eq!(ws.span.hi.0, 8, "comment-whitespace hi (covers full `/* c */`)");
}

#[test]
fn newline_always_emitted_even_in_default_mode() {
    let src = "a\nb";
    let tokens = tokenize(src);

    let nl_count = tokens.iter().filter(|t| t.kind == PpTokenKind::Newline).count();
    assert_eq!(nl_count, 1, "expected one Newline, got {tokens:?}");
}
