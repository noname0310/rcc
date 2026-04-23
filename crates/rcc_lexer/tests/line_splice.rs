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
    // Post task 03-lex/04 `abcdef` (with a physical splice in the middle)
    // is recognised as a single identifier; the splice is invisible
    // to the consumer. The Ident's physical span must still cover the
    // full byte range of `abc\\\ndef`.
    let src = "abc\\\ndef";
    let file = FileId(0);
    let tokens: Vec<_> = rcc_lexer::tokenize(file, src).collect();

    assert_eq!(tokens.len(), 1, "expected a single Ident token, got {tokens:?}");
    let tok = tokens[0];
    assert_eq!(tok.kind, rcc_lexer::PpTokenKind::Ident);
    assert_eq!(tok.span.lo.0, 0);
    assert_eq!(tok.span.hi.0, src.len() as u32, "ident span must cover physical bytes");
}

#[test]
fn tokenizer_directive_splice_smoke() {
    // `#define FOO \\\n bar` ⇒ after phase-2 splicing the logical text
    // is `#define FOO  bar`. Identifier tokens must see `define`, `FOO`,
    // and `bar` with physical spans anchored in the pre-splice source.
    let src = "#define FOO \\\n bar";
    let file = FileId(0);
    let tokens: Vec<_> = rcc_lexer::tokenize(file, src).collect();

    // Each identifier token's *logical* text is recovered by stripping
    // backslash-newline pairs from the physical slice (phase-2 splicing
    // is invisible at the span level).
    let idents: Vec<String> = tokens
        .iter()
        .filter(|t| t.kind == rcc_lexer::PpTokenKind::Ident)
        .map(|t| {
            let s = &src[t.span.lo.0 as usize..t.span.hi.0 as usize];
            s.replace("\\\r\n", "").replace("\\\n", "")
        })
        .collect();

    assert!(idents.iter().any(|s| s == "define"), "expected `define`, got {idents:?}");
    assert!(idents.iter().any(|s| s == "FOO"), "expected `FOO`, got {idents:?}");
    assert!(idents.iter().any(|s| s == "bar"), "expected `bar`, got {idents:?}");
}
