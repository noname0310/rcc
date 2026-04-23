use rcc_lexer::LineSpliceCursor;
use rcc_span::FileId;

// ── LineSpliceCursor unit tests ─────────────────────────────────────

#[test]
fn splice_joins_identifier_chars() {
    let mut c = LineSpliceCursor::new("abc\\\ndef");
    let mut chars = Vec::new();
    while let Some(ch) = c.bump() {
        chars.push(ch);
    }
    assert_eq!(chars, vec!['a', 'b', 'c', 'd', 'e', 'f']);
}

#[test]
fn splice_physical_offset_spans_backslash_newline() {
    let mut c = LineSpliceCursor::new("abc\\\ndef");
    // a(0) b(1) c(2) \(3) \n(4) d(5) e(6) f(7)
    c.bump(); // a → offset 1
    c.bump(); // b → offset 2
    c.bump(); // c → offset 3
    let before_d = c.offset(); // 3 — physical pos before the splice
    c.bump(); // d — skips \\\n then consumes d → offset 6
    let after_d = c.offset();
    assert_eq!(before_d, 3);
    assert_eq!(after_d, 6);
}

#[test]
fn splice_at_start() {
    let mut c = LineSpliceCursor::new("\\\nabc");
    assert_eq!(c.first(), Some('a'));
    c.bump(); // skips \\\n, bumps 'a'
    assert_eq!(c.offset(), 3);
    assert_eq!(c.first(), Some('b'));
}

#[test]
fn splice_at_end_is_eof() {
    let mut c = LineSpliceCursor::new("x\\\n");
    assert_eq!(c.first(), Some('x'));
    c.bump(); // 'x'
    assert!(c.is_eof());
    assert_eq!(c.bump(), None);
}

#[test]
fn no_splice_plain_backslash() {
    let mut c = LineSpliceCursor::new("a\\b");
    assert_eq!(c.bump(), Some('a'));
    assert_eq!(c.bump(), Some('\\'));
    assert_eq!(c.bump(), Some('b'));
    assert!(c.is_eof());
}

#[test]
fn consecutive_splices() {
    let mut c = LineSpliceCursor::new("a\\\n\\\nb");
    assert_eq!(c.bump(), Some('a'));
    let before = c.offset();
    assert_eq!(c.bump(), Some('b'));
    let after = c.offset();
    // a(0) \(1) \n(2) \(3) \n(4) b(5)
    assert_eq!(before, 1);
    assert_eq!(after, 6);
}

#[test]
fn crlf_splice() {
    let mut c = LineSpliceCursor::new("ab\\\r\ncd");
    let mut chars = Vec::new();
    while let Some(ch) = c.bump() {
        chars.push(ch);
    }
    assert_eq!(chars, vec!['a', 'b', 'c', 'd']);
}

#[test]
fn peek_sees_past_splice() {
    let c = LineSpliceCursor::new("a\\\nb");
    assert_eq!(c.first(), Some('a'));
    assert_eq!(c.second(), Some('b'));
    assert_eq!(c.peek_at(0), Some('a'));
    assert_eq!(c.peek_at(1), Some('b'));
    assert_eq!(c.peek_at(2), None);
}

#[test]
fn eat_while_across_splice() {
    let mut c = LineSpliceCursor::new("aa\\\naa!");
    c.eat_while(|ch| ch == 'a');
    assert_eq!(c.first(), Some('!'));
    // a(0) a(1) \(2) \n(3) a(4) a(5) !(6)
    assert_eq!(c.offset(), 6);
}

#[test]
fn bump_while_across_splice() {
    let mut c = LineSpliceCursor::new("12\\\n34x");
    let n = c.bump_while(|ch| ch.is_ascii_digit());
    assert_eq!(n, 4);
    assert_eq!(c.first(), Some('x'));
}

#[test]
fn bump_if_across_splice() {
    let mut c = LineSpliceCursor::new("\\\nx");
    assert!(c.bump_if(|ch| ch == 'x'));
    assert!(c.is_eof());
}

#[test]
fn only_splice_input_is_eof() {
    let c = LineSpliceCursor::new("\\\n");
    assert!(c.is_eof());
    assert_eq!(c.first(), None);
}

#[test]
fn backslash_not_followed_by_newline_is_literal() {
    let mut c = LineSpliceCursor::new("\\t");
    assert_eq!(c.bump(), Some('\\'));
    assert_eq!(c.bump(), Some('t'));
    assert!(c.is_eof());
}

// ── Tokenizer-level span tests ──────────────────────────────────────

#[test]
fn tokenizer_splice_span_covers_physical_range() {
    let src = "abc\\\ndef";
    let file = FileId(0);
    let tokens: Vec<_> = rcc_lexer::tokenize(file, src).collect();

    // Current stub emits one Unknown token per logical char.
    // The splice is invisible, so we get 6 tokens (a, b, c, d, e, f).
    assert_eq!(tokens.len(), 6, "expected 6 logical chars, got {}", tokens.len());

    // Token 'd' crosses the splice: its span must start at the
    // backslash (byte 3) and end after 'd' (byte 6).
    let d_tok = &tokens[3];
    assert_eq!(d_tok.span.lo.0, 3, "d token lo");
    assert_eq!(d_tok.span.hi.0, 6, "d token hi");

    // Full physical range of all tokens: [0, 8).
    let first_lo = tokens.first().unwrap().span.lo.0;
    let last_hi = tokens.last().unwrap().span.hi.0;
    assert_eq!(first_lo, 0);
    assert_eq!(last_hi, src.len() as u32);
}

#[test]
fn tokenizer_directive_splice_smoke() {
    let src = "#define FOO \\\n bar";
    let file = FileId(0);
    let tokens: Vec<_> = rcc_lexer::tokenize(file, src).collect();

    // Collect logical characters from spans.
    let logical: String = tokens
        .iter()
        .filter_map(|t| {
            let lo = t.span.lo.0 as usize;
            let hi = t.span.hi.0 as usize;
            // For splice-crossing tokens the slice may contain \\\n,
            // so just take the last char (the actual logical char).
            src.get(lo..hi).and_then(|s| s.chars().rfind(|&c| c != '\\' && c != '\n' && c != '\r'))
        })
        .collect();

    // After splicing: "#define FOO  bar" (the \\\n removed, space preserved).
    assert!(logical.contains("FOO"), "directive name present: {logical}");
    assert!(logical.contains("bar"), "macro body present: {logical}");
}
