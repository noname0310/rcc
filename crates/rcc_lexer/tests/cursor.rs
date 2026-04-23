use rcc_lexer::Cursor;

// ── Empty input ──────────────────────────────────────────────────────

#[test]
fn empty_input_is_eof() {
    let c = Cursor::new("");
    assert!(c.is_eof());
    assert_eq!(c.offset(), 0);
    assert_eq!(c.rest(), "");
}

#[test]
fn empty_input_first_second_peek_at() {
    let c = Cursor::new("");
    assert_eq!(c.first(), None);
    assert_eq!(c.second(), None);
    assert_eq!(c.peek_at(0), None);
    assert_eq!(c.peek_at(5), None);
}

#[test]
fn empty_input_bump_returns_none() {
    let mut c = Cursor::new("");
    assert_eq!(c.bump(), None);
    assert!(c.is_eof());
}

#[test]
fn empty_input_bump_if_returns_false() {
    let mut c = Cursor::new("");
    assert!(!c.bump_if(|_| true));
}

#[test]
fn empty_input_bump_while_returns_zero() {
    let mut c = Cursor::new("");
    assert_eq!(c.bump_while(|_| true), 0);
}

#[test]
fn empty_input_eat_while_is_noop() {
    let mut c = Cursor::new("");
    c.eat_while(|_| true);
    assert!(c.is_eof());
    assert_eq!(c.offset(), 0);
}

// ── Single-byte ASCII ────────────────────────────────────────────────

#[test]
fn single_ascii_char() {
    let mut c = Cursor::new("x");
    assert!(!c.is_eof());
    assert_eq!(c.first(), Some('x'));
    assert_eq!(c.second(), None);
    assert_eq!(c.offset(), 0);

    assert_eq!(c.bump(), Some('x'));
    assert!(c.is_eof());
    assert_eq!(c.offset(), 1);
    assert_eq!(c.bump(), None);
}

// ── peek_at ──────────────────────────────────────────────────────────

#[test]
fn peek_at_various_offsets() {
    let c = Cursor::new("abcd");
    assert_eq!(c.peek_at(0), Some('a'));
    assert_eq!(c.peek_at(1), Some('b'));
    assert_eq!(c.peek_at(2), Some('c'));
    assert_eq!(c.peek_at(3), Some('d'));
    assert_eq!(c.peek_at(4), None);
}

#[test]
fn peek_at_after_bump() {
    let mut c = Cursor::new("abcd");
    c.bump(); // consume 'a'
    assert_eq!(c.peek_at(0), Some('b'));
    assert_eq!(c.peek_at(2), Some('d'));
    assert_eq!(c.peek_at(3), None);
}

// ── bump_if ──────────────────────────────────────────────────────────

#[test]
fn bump_if_matching() {
    let mut c = Cursor::new("abc");
    assert!(c.bump_if(|ch| ch == 'a'));
    assert_eq!(c.offset(), 1);
    assert_eq!(c.first(), Some('b'));
}

#[test]
fn bump_if_not_matching() {
    let mut c = Cursor::new("abc");
    assert!(!c.bump_if(|ch| ch == 'z'));
    assert_eq!(c.offset(), 0);
    assert_eq!(c.first(), Some('a'));
}

// ── bump_while ───────────────────────────────────────────────────────

#[test]
fn bump_while_consumes_matching_prefix() {
    let mut c = Cursor::new("aaabbc");
    let n = c.bump_while(|ch| ch == 'a');
    assert_eq!(n, 3);
    assert_eq!(c.first(), Some('b'));
}

#[test]
fn bump_while_no_match_returns_zero() {
    let mut c = Cursor::new("xyz");
    let n = c.bump_while(|ch| ch == 'a');
    assert_eq!(n, 0);
    assert_eq!(c.offset(), 0);
}

#[test]
fn bump_while_consumes_all() {
    let mut c = Cursor::new("aaa");
    let n = c.bump_while(|ch| ch == 'a');
    assert_eq!(n, 3);
    assert!(c.is_eof());
}

// ── eat_while ────────────────────────────────────────────────────────

#[test]
fn eat_while_stops_at_non_matching() {
    let mut c = Cursor::new("111abc");
    c.eat_while(|ch| ch.is_ascii_digit());
    assert_eq!(c.offset(), 3);
    assert_eq!(c.first(), Some('a'));
}

// ── Multi-byte UTF-8 ─────────────────────────────────────────────────

#[test]
fn multibyte_offset_tracks_bytes() {
    // '€' is 3 bytes in UTF-8 (U+20AC)
    let mut c = Cursor::new("€x");
    assert_eq!(c.offset(), 0);
    assert_eq!(c.first(), Some('€'));

    c.bump(); // consume '€'
    assert_eq!(c.offset(), 3);
    assert_eq!(c.first(), Some('x'));

    c.bump(); // consume 'x'
    assert_eq!(c.offset(), 4);
    assert!(c.is_eof());
}

#[test]
fn multibyte_peek_at() {
    // '한' (U+D55C) = 3 bytes, '글' (U+AE00) = 3 bytes
    let c = Cursor::new("한글!");
    assert_eq!(c.peek_at(0), Some('한'));
    assert_eq!(c.peek_at(1), Some('글'));
    assert_eq!(c.peek_at(2), Some('!'));
    assert_eq!(c.peek_at(3), None);
}

#[test]
fn multibyte_bump_if() {
    let mut c = Cursor::new("über");
    assert!(c.bump_if(|ch| ch == 'ü'));
    assert_eq!(c.offset(), 2); // 'ü' is 2 bytes in UTF-8
    assert_eq!(c.first(), Some('b'));
}

#[test]
fn multibyte_bump_while() {
    // Four 4-byte emoji characters
    let src = "😀😁😂x";
    let mut c = Cursor::new(src);
    let n = c.bump_while(|ch| !ch.is_ascii());
    assert_eq!(n, 3);
    assert_eq!(c.offset(), 12); // 3 × 4 bytes
    assert_eq!(c.first(), Some('x'));
}

#[test]
fn mixed_ascii_multibyte_full_walk() {
    let src = "aé日🦀";
    let mut c = Cursor::new(src);

    let mut offsets = Vec::new();
    while c.bump().is_some() {
        offsets.push(c.offset());
    }
    // 'a' = 1, 'é' = 2, '日' = 3, '🦀' = 4 => cumulative: 1, 3, 6, 10
    assert_eq!(offsets, vec![1, 3, 6, 10]);
    assert_eq!(c.offset(), src.len());
}

#[test]
fn rest_returns_unconsumed_slice() {
    let mut c = Cursor::new("hello");
    c.bump();
    c.bump();
    assert_eq!(c.rest(), "llo");
}

#[test]
fn first_and_second_consistency() {
    let c = Cursor::new("ab");
    assert_eq!(c.first(), Some('a'));
    assert_eq!(c.second(), Some('b'));
    assert_eq!(c.peek_at(0), c.first());
    assert_eq!(c.peek_at(1), c.second());
}

// ── Proptest roundtrip ───────────────────────────────────────────────

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(1000))]

    #[test]
    fn offset_equals_sum_of_utf8_widths(s in ".*") {
        let mut cursor = Cursor::new(&s);
        let mut expected_offset: usize = 0;
        while let Some(ch) = cursor.bump() {
            expected_offset += ch.len_utf8();
            proptest::prop_assert_eq!(cursor.offset(), expected_offset);
        }
        proptest::prop_assert_eq!(cursor.offset(), s.len());
        proptest::prop_assert!(cursor.is_eof());
    }
}
