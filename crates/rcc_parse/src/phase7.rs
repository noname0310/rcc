//! C99 §5.1.1.2 phase 7: convert a preprocessed pp-token stream into the
//! parser-level [`Token`] type.
//!
//! This module only implements the *dispatcher*. Per-literal decoding and
//! keyword classification live in sibling tasks (05-02 through 05-06) and
//! currently land as stub values; the match arms are wired so that adding a
//! new [`PpTokenKind`] variant produces a compile error until it is handled
//! here.
//!
//! Whitespace, newlines, the lexer's EOF sentinel, and [`PpTokenKind::Unknown`]
//! are intentionally dropped — the parser consumes a stream of *parser*
//! tokens only, and lexical categories that do not survive phase 7 have no
//! representation in [`TokenKind`].

use rcc_lexer::{PpNumberKind, PpToken, PpTokenKind};
use rcc_session::Session;
use rcc_span::Span;

use crate::token::{
    CharLiteral, FloatLiteral, FloatSuffix, IntLiteral, IntSuffix, StringLiteral, Token, TokenKind,
};

/// Convert a slice of pp-tokens into a fresh `Vec<Token>`.
///
/// Whitespace, newlines, the lexer's EOF sentinel, and any `Unknown`
/// tokens (already diagnosed by the lexer) are silently dropped. Adjacent
/// string-literal concatenation is *not* performed here — task 05-06
/// layers that on top once per-literal decoding lands.
pub fn convert(session: &mut Session, pp: &[PpToken]) -> Vec<Token> {
    let mut out = Vec::with_capacity(pp.len());
    for tok in pp {
        if let Some(t) = pp_to_token(session, *tok) {
            out.push(t);
        }
    }
    out
}

/// Convert a single pp-token to zero or one parser tokens.
///
/// Returns `None` for pp-tokens that have no parser-level representation
/// (whitespace, newlines, EOF, `Unknown`, or — defensively — a stray
/// `HeaderName` that escaped `#include` processing in the preprocessor).
pub fn pp_to_token(session: &mut Session, pp: PpToken) -> Option<Token> {
    let kind = match pp.kind {
        PpTokenKind::Ident => {
            let sym = intern_span(session, pp.span);
            // TODO(05-02): run keyword classification on `sym` here so that
            // reserved words become `TokenKind::Keyword(_)` instead of
            // identifiers. Until that lands every ident — reserved or not —
            // goes through as `Ident(sym)`.
            TokenKind::Ident(sym)
        }
        PpTokenKind::PpNumber(PpNumberKind::Integer) => {
            // TODO(05-03): decode the integer literal from the span text,
            // detect the `u`/`l`/`ll` suffix combination, and diagnose
            // overflow per C99 §6.4.4.1.
            TokenKind::IntLit(IntLiteral { value: 0, suffix: IntSuffix::None })
        }
        PpTokenKind::PpNumber(PpNumberKind::Float) => {
            // TODO(05-04): decode the floating constant (decimal +
            // hex-float) and its `f`/`l` suffix per C99 §6.4.4.2.
            TokenKind::FloatLit(FloatLiteral { value: 0.0, suffix: FloatSuffix::None })
        }
        PpTokenKind::CharConst { enc } => {
            // TODO(05-05): decode the character constant including every
            // escape sequence family (simple, octal, hex, universal) per
            // C99 §6.4.4.4.
            TokenKind::CharLit(CharLiteral { value: 0, encoding: enc })
        }
        PpTokenKind::StringLit { enc } => {
            // TODO(05-06): decode escape sequences and concatenate adjacent
            // string literals (including encoding-prefix compatibility) per
            // C99 §6.4.5. The current stub emits an empty byte payload so
            // downstream code can still pattern-match on the variant.
            TokenKind::StringLit(StringLiteral { bytes: Vec::new(), encoding: enc })
        }
        PpTokenKind::Punct(p) => TokenKind::Punct(p),
        PpTokenKind::HeaderName => {
            // `header-name` pp-tokens are only produced inside the
            // `#include` directive and consumed by the preprocessor. Seeing
            // one here means the preprocessor let it leak, which is an
            // internal invariant violation rather than user-facing C code
            // being wrong — report it loudly and drop the token.
            session
                .handler
                .struct_err(
                    pp.span,
                    "header-name pp-token reached phase-7 conversion outside #include",
                )
                .note("this is an internal invariant of the preprocessor")
                .emit();
            return None;
        }
        PpTokenKind::Whitespace
        | PpTokenKind::Newline
        | PpTokenKind::Eof
        | PpTokenKind::Unknown => return None,
    };
    Some(Token { kind, span: pp.span })
}

/// Intern the source slice covered by `span` using the session's interner.
fn intern_span(session: &mut Session, span: Span) -> rcc_span::Symbol {
    let text = {
        let sm = session.source_map.read().expect("source map poisoned");
        let file = sm.file(span.file);
        file.src[span.lo.0 as usize..span.hi.0 as usize].to_owned()
    };
    session.interner.intern(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcc_lexer::{Punct, StringEncoding};
    use rcc_session::Session;
    use rcc_span::{BytePos, FileId, Span};
    use std::sync::Arc;

    fn mk_session(src: &str) -> (Session, FileId) {
        let (sess, _cap) = Session::for_test();
        let fid =
            sess.source_map.write().unwrap().add_file("t.c".into(), Arc::from(src.to_owned()));
        (sess, fid)
    }

    fn tok(kind: PpTokenKind, file: FileId, lo: u32, hi: u32) -> PpToken {
        PpToken {
            kind,
            span: Span::new(file, BytePos(lo), BytePos(hi)),
            leading_ws: false,
            at_line_start: false,
        }
    }

    /// Compile-time exhaustiveness witness: adding a new `PpTokenKind`
    /// variant must force a matching arm in `pp_to_token` (no wildcard
    /// there) and this helper (no wildcard here either).
    #[allow(dead_code)]
    fn exhaustive_kind_check(k: PpTokenKind) -> &'static str {
        match k {
            PpTokenKind::HeaderName => "header",
            PpTokenKind::Ident => "ident",
            PpTokenKind::PpNumber(PpNumberKind::Integer) => "int",
            PpTokenKind::PpNumber(PpNumberKind::Float) => "float",
            PpTokenKind::CharConst { .. } => "char",
            PpTokenKind::StringLit { .. } => "string",
            PpTokenKind::Punct(_) => "punct",
            PpTokenKind::Newline => "newline",
            PpTokenKind::Whitespace => "ws",
            PpTokenKind::Unknown => "unknown",
            PpTokenKind::Eof => "eof",
        }
    }

    #[test]
    fn ident_is_interned_and_span_preserved() {
        let (mut sess, fid) = mk_session("foo");
        let pp = tok(PpTokenKind::Ident, fid, 0, 3);
        let t = pp_to_token(&mut sess, pp).expect("ident converts");
        assert_eq!(t.span, pp.span, "span preserved 1:1");
        match t.kind {
            TokenKind::Ident(sym) => assert_eq!(sess.interner.get(sym), "foo"),
            other => panic!("expected Ident, got {other:?}"),
        }
    }

    #[test]
    fn punct_passes_through() {
        let (mut sess, fid) = mk_session("+");
        let pp = tok(PpTokenKind::Punct(Punct::Plus), fid, 0, 1);
        let t = pp_to_token(&mut sess, pp).expect("punct converts");
        assert_eq!(t.kind, TokenKind::Punct(Punct::Plus));
        assert_eq!(t.span, pp.span);
    }

    #[test]
    fn integer_pp_number_becomes_intlit_stub() {
        let (mut sess, fid) = mk_session("42");
        let pp = tok(PpTokenKind::PpNumber(PpNumberKind::Integer), fid, 0, 2);
        let t = pp_to_token(&mut sess, pp).expect("int converts");
        match t.kind {
            TokenKind::IntLit(lit) => {
                // Stub values; real decoding lives in 05-03.
                assert_eq!(lit.value, 0);
                assert_eq!(lit.suffix, IntSuffix::None);
            }
            other => panic!("expected IntLit, got {other:?}"),
        }
    }

    #[test]
    fn float_pp_number_becomes_floatlit_stub() {
        let (mut sess, fid) = mk_session("1.0");
        let pp = tok(PpTokenKind::PpNumber(PpNumberKind::Float), fid, 0, 3);
        let t = pp_to_token(&mut sess, pp).expect("float converts");
        match t.kind {
            TokenKind::FloatLit(lit) => {
                assert_eq!(lit.value, 0.0);
                assert_eq!(lit.suffix, FloatSuffix::None);
            }
            other => panic!("expected FloatLit, got {other:?}"),
        }
    }

    #[test]
    fn char_const_becomes_charlit_stub_with_encoding() {
        let (mut sess, fid) = mk_session("L'a'");
        let pp = tok(PpTokenKind::CharConst { enc: StringEncoding::Wide }, fid, 0, 4);
        let t = pp_to_token(&mut sess, pp).expect("char converts");
        match t.kind {
            TokenKind::CharLit(lit) => {
                assert_eq!(lit.value, 0);
                assert_eq!(lit.encoding, StringEncoding::Wide);
            }
            other => panic!("expected CharLit, got {other:?}"),
        }
    }

    #[test]
    fn string_lit_becomes_stringlit_stub_with_encoding() {
        let (mut sess, fid) = mk_session("\"hi\"");
        let pp = tok(PpTokenKind::StringLit { enc: StringEncoding::Utf8 }, fid, 0, 4);
        let t = pp_to_token(&mut sess, pp).expect("string converts");
        match t.kind {
            TokenKind::StringLit(lit) => {
                assert!(lit.bytes.is_empty(), "05-06 fills bytes in");
                assert_eq!(lit.encoding, StringEncoding::Utf8);
            }
            other => panic!("expected StringLit, got {other:?}"),
        }
    }

    #[test]
    fn whitespace_newline_eof_unknown_are_skipped() {
        let (mut sess, fid) = mk_session("   ");
        for kind in
            [PpTokenKind::Whitespace, PpTokenKind::Newline, PpTokenKind::Eof, PpTokenKind::Unknown]
        {
            let pp = tok(kind, fid, 0, 0);
            assert!(pp_to_token(&mut sess, pp).is_none(), "{kind:?} must be dropped");
        }
    }

    #[test]
    fn header_name_outside_include_emits_error_and_is_dropped() {
        let (mut sess, cap) = Session::for_test();
        let fid = sess
            .source_map
            .write()
            .unwrap()
            .add_file("t.c".into(), Arc::from("<stdio.h>".to_owned()));
        let pp = tok(PpTokenKind::HeaderName, fid, 0, 9);
        let t = pp_to_token(&mut sess, pp);
        assert!(t.is_none(), "header-name must not survive as a parser token");
        assert_eq!(sess.handler.error_count(), 1, "exactly one error emitted");
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("header-name"),
            "message should mention header-name, got {:?}",
            diags[0].message
        );
    }

    #[test]
    fn convert_drops_whitespace_and_preserves_order() {
        let (mut sess, fid) = mk_session("a + b");
        let stream = [
            tok(PpTokenKind::Ident, fid, 0, 1),
            tok(PpTokenKind::Whitespace, fid, 1, 2),
            tok(PpTokenKind::Punct(Punct::Plus), fid, 2, 3),
            tok(PpTokenKind::Whitespace, fid, 3, 4),
            tok(PpTokenKind::Ident, fid, 4, 5),
            tok(PpTokenKind::Newline, fid, 5, 5),
            tok(PpTokenKind::Eof, fid, 5, 5),
        ];
        let out = convert(&mut sess, &stream);
        assert_eq!(out.len(), 3, "whitespace/newline/eof dropped: {out:?}");
        assert!(matches!(out[0].kind, TokenKind::Ident(_)));
        assert_eq!(out[1].kind, TokenKind::Punct(Punct::Plus));
        assert!(matches!(out[2].kind, TokenKind::Ident(_)));
        for (i, expected_span) in [(0, (0u32, 1u32)), (1, (2, 3)), (2, (4, 5))] {
            assert_eq!(out[i].span.lo.0, expected_span.0);
            assert_eq!(out[i].span.hi.0, expected_span.1);
        }
    }
}
