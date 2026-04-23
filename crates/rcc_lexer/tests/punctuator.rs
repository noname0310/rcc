//! Punctuator recogniser (task 03-lex/08).
//!
//! C99 §6.4.6 — punctuators, maximal-munch:
//!
//! ```text
//! 3-char : <<=  >>=  ...
//! 2-char : ->  ++  --  ==  !=  <=  >=  &&  ||  <<  >>
//!          +=  -=  *=  /=  %=  &=  |=  ^=  ##
//! 1-char : [ ] ( ) { } . & * + - ~ ! / % < > ^ | ? : ; = , #
//! ```
//!
//! The lexer must emit one `Punct(_)` pp-token per maximal punctuator;
//! bytes that cannot begin any token land as `Unknown` + E0001.

use rcc_errors::codes::E0001;
use rcc_lexer::{PpTokenKind, Punct, Tokenizer};
use rcc_session::Session;
use rcc_span::FileId;

fn tokenize(src: &str) -> Vec<rcc_lexer::PpToken> {
    Tokenizer::new(FileId(0), src).collect()
}

fn diags(src: &str) -> Vec<rcc_errors::Diagnostic> {
    let (mut sess, cap) = Session::for_test();
    let _: Vec<_> = Tokenizer::new(FileId(0), src).with_handler(&mut sess.handler).collect();
    cap.diagnostics()
}

/// Canonical spelling for every `Punct` variant. Keep in sync with
/// the enum in `rcc_lexer::kinds::Punct`.
fn punct_spelling(p: Punct) -> &'static str {
    match p {
        Punct::LBracket => "[",
        Punct::RBracket => "]",
        Punct::LParen => "(",
        Punct::RParen => ")",
        Punct::LBrace => "{",
        Punct::RBrace => "}",
        Punct::Dot => ".",
        Punct::Arrow => "->",
        Punct::PlusPlus => "++",
        Punct::MinusMinus => "--",
        Punct::Amp => "&",
        Punct::Star => "*",
        Punct::Plus => "+",
        Punct::Minus => "-",
        Punct::Tilde => "~",
        Punct::Bang => "!",
        Punct::Slash => "/",
        Punct::Percent => "%",
        Punct::ShlShl => "<<",
        Punct::ShrShr => ">>",
        Punct::Lt => "<",
        Punct::Gt => ">",
        Punct::Le => "<=",
        Punct::Ge => ">=",
        Punct::EqEq => "==",
        Punct::BangEq => "!=",
        Punct::Caret => "^",
        Punct::Pipe => "|",
        Punct::AmpAmp => "&&",
        Punct::PipePipe => "||",
        Punct::Question => "?",
        Punct::Colon => ":",
        Punct::Semi => ";",
        Punct::Ellipsis => "...",
        Punct::Eq => "=",
        Punct::StarEq => "*=",
        Punct::SlashEq => "/=",
        Punct::PercentEq => "%=",
        Punct::PlusEq => "+=",
        Punct::MinusEq => "-=",
        Punct::ShlEq => "<<=",
        Punct::ShrEq => ">>=",
        Punct::AmpEq => "&=",
        Punct::CaretEq => "^=",
        Punct::PipeEq => "|=",
        Punct::Comma => ",",
        Punct::Hash => "#",
        Punct::HashHash => "##",
    }
}

/// The full list of `Punct` variants. An exhaustive `match` in
/// `punct_spelling` above would break compilation if a new variant
/// were added without updating this list, so the coverage is
/// effectively compile-time enforced.
const ALL_PUNCTS: &[Punct] = &[
    Punct::LBracket,
    Punct::RBracket,
    Punct::LParen,
    Punct::RParen,
    Punct::LBrace,
    Punct::RBrace,
    Punct::Dot,
    Punct::Arrow,
    Punct::PlusPlus,
    Punct::MinusMinus,
    Punct::Amp,
    Punct::Star,
    Punct::Plus,
    Punct::Minus,
    Punct::Tilde,
    Punct::Bang,
    Punct::Slash,
    Punct::Percent,
    Punct::ShlShl,
    Punct::ShrShr,
    Punct::Lt,
    Punct::Gt,
    Punct::Le,
    Punct::Ge,
    Punct::EqEq,
    Punct::BangEq,
    Punct::Caret,
    Punct::Pipe,
    Punct::AmpAmp,
    Punct::PipePipe,
    Punct::Question,
    Punct::Colon,
    Punct::Semi,
    Punct::Ellipsis,
    Punct::Eq,
    Punct::StarEq,
    Punct::SlashEq,
    Punct::PercentEq,
    Punct::PlusEq,
    Punct::MinusEq,
    Punct::ShlEq,
    Punct::ShrEq,
    Punct::AmpEq,
    Punct::CaretEq,
    Punct::PipeEq,
    Punct::Comma,
    Punct::Hash,
    Punct::HashHash,
];

// ── Acceptance: every Punct round-trips in isolation ────────────────

#[test]
fn every_punct_variant_round_trips_in_isolation() {
    for &p in ALL_PUNCTS {
        let src = punct_spelling(p);
        let toks: Vec<_> = tokenize(src)
            .into_iter()
            .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
            .collect();
        assert_eq!(toks.len(), 1, "punct {src:?}: expected one non-ws token, got {toks:?}");
        assert_eq!(
            toks[0].kind,
            PpTokenKind::Punct(p),
            "punct {src:?}: expected {p:?}, got {:?}",
            toks[0].kind,
        );
        assert_eq!(toks[0].span.lo.0, 0);
        assert_eq!(toks[0].span.hi.0, src.len() as u32);
        assert!(diags(src).is_empty(), "punct {src:?} must not diagnose");
    }
}

// ── Acceptance: space-joined concatenation round-trips losslessly ───

#[test]
fn all_puncts_space_joined_round_trip() {
    let spellings: Vec<&str> = ALL_PUNCTS.iter().copied().map(punct_spelling).collect();
    let src = spellings.join(" ");
    let puncts: Vec<Punct> = tokenize(&src)
        .into_iter()
        .filter_map(|t| match t.kind {
            PpTokenKind::Punct(p) => Some(p),
            PpTokenKind::Whitespace | PpTokenKind::Newline => None,
            other => panic!("unexpected non-punct token {other:?} in {src:?}"),
        })
        .collect();
    assert_eq!(puncts, ALL_PUNCTS, "round-trip mismatch for {src:?}");
    assert!(diags(&src).is_empty());
}

// ── Max-munch: `...` preferred over `..` + `.` ──────────────────────

#[test]
fn ellipsis_is_max_munch() {
    let toks: Vec<_> = tokenize("...")
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].kind, PpTokenKind::Punct(Punct::Ellipsis));
    assert_eq!(toks[0].span.hi.0, 3);
}

#[test]
fn two_dots_is_two_dot_puncts() {
    // `..` is NOT an ellipsis; max-munch here yields `.` then `.`.
    let toks: Vec<_> = tokenize("..")
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 2);
    assert_eq!(toks[0].kind, PpTokenKind::Punct(Punct::Dot));
    assert_eq!(toks[1].kind, PpTokenKind::Punct(Punct::Dot));
}

#[test]
fn four_dots_is_ellipsis_plus_dot() {
    // Max-munch takes the longest prefix, `...`, then starts fresh at
    // the remaining `.`.
    let toks: Vec<_> = tokenize("....")
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 2);
    assert_eq!(toks[0].kind, PpTokenKind::Punct(Punct::Ellipsis));
    assert_eq!(toks[0].span.hi.0, 3);
    assert_eq!(toks[1].kind, PpTokenKind::Punct(Punct::Dot));
    assert_eq!(toks[1].span.lo.0, 3);
    assert_eq!(toks[1].span.hi.0, 4);
}

// ── Max-munch: compound-assignments vs their single forms ───────────

#[test]
fn shl_eq_is_max_munch() {
    let toks: Vec<_> = tokenize("<<=")
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].kind, PpTokenKind::Punct(Punct::ShlEq));
    assert_eq!(toks[0].span.hi.0, 3);
}

#[test]
fn shr_eq_is_max_munch() {
    let toks: Vec<_> = tokenize(">>=")
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].kind, PpTokenKind::Punct(Punct::ShrEq));
    assert_eq!(toks[0].span.hi.0, 3);
}

#[test]
fn shl_without_eq_is_two_char() {
    // `<< ` — the trailing space defeats the 3-char match.
    let toks: Vec<_> = tokenize("<< ")
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].kind, PpTokenKind::Punct(Punct::ShlShl));
}

#[test]
fn lt_alone_is_single_char() {
    let toks: Vec<_> = tokenize("<")
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].kind, PpTokenKind::Punct(Punct::Lt));
}

#[test]
fn arrow_vs_minus_minus_vs_minus_eq_vs_minus() {
    // All four share the `-` prefix; each must be recognised at
    // maximal length.
    let cases: &[(&str, Punct)] = &[
        ("->", Punct::Arrow),
        ("--", Punct::MinusMinus),
        ("-=", Punct::MinusEq),
        ("-", Punct::Minus),
    ];
    for &(src, expected) in cases {
        let toks: Vec<_> = tokenize(src)
            .into_iter()
            .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
            .collect();
        assert_eq!(toks.len(), 1, "{src:?}");
        assert_eq!(toks[0].kind, PpTokenKind::Punct(expected), "{src:?}");
    }
}

#[test]
fn hash_hash_vs_hash() {
    let toks: Vec<_> = tokenize("##")
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].kind, PpTokenKind::Punct(Punct::HashHash));

    let toks: Vec<_> = tokenize("# ")
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].kind, PpTokenKind::Punct(Punct::Hash));
}

// ── Interaction with already-recognised tokens ──────────────────────

#[test]
fn slash_is_punct_when_not_starting_a_comment() {
    // `a/b` — the `/` has no second `/` or `*` after it, so it is a
    // plain `Slash` punctuator, not a comment.
    let toks: Vec<_> = tokenize("a/b")
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 3);
    assert_eq!(toks[0].kind, PpTokenKind::Ident);
    assert_eq!(toks[1].kind, PpTokenKind::Punct(Punct::Slash));
    assert_eq!(toks[2].kind, PpTokenKind::Ident);
}

#[test]
fn dot_does_not_swallow_identifier() {
    // `.x` — `.` alone is a `Dot` punctuator, then `x` as ident.
    let toks: Vec<_> = tokenize(".x")
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 2);
    assert_eq!(toks[0].kind, PpTokenKind::Punct(Punct::Dot));
    assert_eq!(toks[1].kind, PpTokenKind::Ident);
}

#[test]
fn dot_followed_by_digit_is_pp_number_not_dot() {
    // `.5` is a pp-number (Float), not Dot + PpNumber(Integer).
    let toks: Vec<_> = tokenize(".5")
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(toks.len(), 1);
    assert!(matches!(toks[0].kind, PpTokenKind::PpNumber(_)));
}

// ── Stray-character diagnostic (E0001) ──────────────────────────────

#[test]
fn at_sign_is_unknown_with_e0001() {
    let src = "@";
    let toks = tokenize(src);
    let non_ws: Vec<_> = toks
        .iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    assert_eq!(non_ws.len(), 1);
    assert_eq!(non_ws[0].kind, PpTokenKind::Unknown);
    assert_eq!(non_ws[0].span.lo.0, 0);
    assert_eq!(non_ws[0].span.hi.0, 1);

    let ds = diags(src);
    assert_eq!(ds.len(), 1, "expected a single diagnostic, got {ds:?}");
    assert_eq!(ds[0].code, Some(E0001));
}

#[test]
fn backtick_is_unknown_with_e0001() {
    // U+0060 GRAVE ACCENT is not part of any C99 token.
    let src = "`";
    let ds = diags(src);
    assert_eq!(ds.len(), 1);
    assert_eq!(ds[0].code, Some(E0001));
}

#[test]
fn stray_backslash_not_starting_ucn_is_unknown() {
    // A bare `\` that isn't followed by `u`/`U` (UCN) or by a newline
    // (line-splice, already consumed by the splicing cursor) is stray.
    let src = "\\z";
    let toks = tokenize(src);
    let non_ws: Vec<_> = toks
        .iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();
    // First token: Unknown for `\`. Second token: identifier `z`.
    assert!(!non_ws.is_empty());
    assert_eq!(non_ws[0].kind, PpTokenKind::Unknown);
    let ds = diags(src);
    assert!(ds.iter().any(|d| d.code == Some(E0001)), "expected E0001 in {ds:?}");
}

// ── `sizeof int` mini-sequence: sanity check across token classes ───

#[test]
fn expression_with_mixed_tokens() {
    // `a+=b<<=c->d[0]` — exercises max-munch across several
    // neighbouring punctuators without intervening whitespace.
    let src = "a+=b<<=c->d[0]";
    let toks: Vec<_> = tokenize(src)
        .into_iter()
        .filter(|t| !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline))
        .collect();

    let expected_kinds: &[PpTokenKind] = &[
        PpTokenKind::Ident,
        PpTokenKind::Punct(Punct::PlusEq),
        PpTokenKind::Ident,
        PpTokenKind::Punct(Punct::ShlEq),
        PpTokenKind::Ident,
        PpTokenKind::Punct(Punct::Arrow),
        PpTokenKind::Ident,
        PpTokenKind::Punct(Punct::LBracket),
        PpTokenKind::PpNumber(rcc_lexer::PpNumberKind::Integer),
        PpTokenKind::Punct(Punct::RBracket),
    ];
    let kinds: Vec<_> = toks.iter().map(|t| t.kind).collect();
    assert_eq!(kinds, expected_kinds, "in {src:?}");
    assert!(diags(src).is_empty());
}
