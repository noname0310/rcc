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

use rcc_errors::{codes, Diagnostic, Label, Level};
use rcc_lexer::{PpNumberKind, PpToken, PpTokenKind};
use rcc_session::Session;
use rcc_span::Span;

use crate::keywords::classify_ident;
use crate::literal::{decode_char_full, decode_float, decode_integer};
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
            // Intern first, then resolve through the interner so that the
            // keyword match runs against the canonical text (C99 §6.4.1).
            // All 37 C99 keywords are real reserved words at every position
            // in the grammar — there is no context-sensitive keyword list
            // like C++'s `override` / `final`, so a one-shot table lookup
            // suffices.
            let sym = intern_span(session, pp.span);
            let text = session.interner.get(sym);
            match classify_ident(text) {
                Some(kw) => TokenKind::Keyword(kw),
                None => TokenKind::Ident(sym),
            }
        }
        PpTokenKind::PpNumber(PpNumberKind::Integer) => {
            // C99 §6.4.4.1 integer-constant decoding. `decode_integer`
            // returns a spanless `Diagnostic` on error — we attach the
            // pp-token's own span here so the diagnostic points at the
            // exact text the user typed. On error we still yield a
            // placeholder `IntLit` (value 0, no suffix) so downstream
            // parser invariants ("every pp-number becomes an IntLit or
            // a FloatLit") hold even when recovery kicks in.
            let text = span_text(session, pp.span);
            match decode_integer(&text) {
                Ok(lit) => TokenKind::IntLit(lit),
                Err(mut diag) => {
                    diag.labels.push(Label {
                        span: pp.span,
                        message: String::new(),
                        primary: true,
                    });
                    session.handler.emit(&diag);
                    TokenKind::IntLit(IntLiteral { value: 0, suffix: IntSuffix::None })
                }
            }
        }
        PpTokenKind::PpNumber(PpNumberKind::Float) => {
            // C99 §6.4.4.2 floating-constant decoding. `decode_float`
            // returns a spanless `Diagnostic` on malformed input; on
            // overflow it returns `Ok` with an infinite value, and we
            // turn that into a W0002 warning here so the user sees the
            // pp-token's own span underlined. On a hard decode error
            // we fall back to a placeholder `0.0` / `None` literal so
            // downstream parser invariants (every pp-number becomes
            // an IntLit or a FloatLit) still hold during recovery.
            let text = span_text(session, pp.span);
            match decode_float(&text) {
                Ok(lit) => {
                    if lit.value.is_infinite() {
                        // Normal pp-number source text cannot spell
                        // infinity, so an infinite decode result is
                        // unambiguously "magnitude ≥ f64::MAX".
                        let diag = float_overflow_warning(pp.span);
                        session.handler.emit(&diag);
                    }
                    TokenKind::FloatLit(lit)
                }
                Err(mut diag) => {
                    diag.labels.push(Label {
                        span: pp.span,
                        message: String::new(),
                        primary: true,
                    });
                    session.handler.emit(&diag);
                    TokenKind::FloatLit(FloatLiteral { value: 0.0, suffix: FloatSuffix::None })
                }
            }
        }
        PpTokenKind::CharConst { enc } => {
            // C99 §6.4.4.4 character-constant decoding. `decode_char_full`
            // also reports whether the constant contained more than one
            // character value — §6.4.4.4p10 makes multi-character
            // constants implementation-defined, so we emit W0003 when
            // the flag trips, with the pp-token's span attached. On a
            // hard decode error we still yield a placeholder `CharLit`
            // (value 0) so downstream parser invariants hold.
            let text = span_text(session, pp.span);
            match decode_char_full(&text, enc) {
                Ok((lit, is_multi)) => {
                    if is_multi {
                        let diag = multi_char_warning(pp.span);
                        session.handler.emit(&diag);
                    }
                    TokenKind::CharLit(lit)
                }
                Err(mut diag) => {
                    diag.labels.push(Label {
                        span: pp.span,
                        message: String::new(),
                        primary: true,
                    });
                    session.handler.emit(&diag);
                    TokenKind::CharLit(CharLiteral { value: 0, encoding: enc })
                }
            }
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

/// Build a W0002 `float literal overflow` warning carrying `span` as the
/// primary label. Lives here (rather than inside `decode_float`) because
/// attaching a span needs a `Span`, and the decoder is deliberately span-
/// agnostic so it stays trivially unit-testable.
fn float_overflow_warning(span: Span) -> Diagnostic {
    Diagnostic {
        level: Level::Warning,
        code: Some(codes::W0002),
        message: "float literal overflow".into(),
        labels: vec![Label { span, message: String::new(), primary: true }],
        notes: Vec::new(),
        help: Vec::new(),
    }
}

/// Build a W0003 `multi-character constant` warning carrying `span` as
/// the primary label. C99 §6.4.4.4p10 makes the value of such a constant
/// implementation-defined; `rcc` packs the constituent bytes big-endian
/// and warns the user that relying on the value is unportable.
fn multi_char_warning(span: Span) -> Diagnostic {
    Diagnostic {
        level: Level::Warning,
        code: Some(codes::W0003),
        message: "multi-character constant".into(),
        labels: vec![Label { span, message: String::new(), primary: true }],
        notes: Vec::new(),
        help: Vec::new(),
    }
}

/// Intern the source slice covered by `span` using the session's interner.
fn intern_span(session: &mut Session, span: Span) -> rcc_span::Symbol {
    let text = span_text(session, span);
    session.interner.intern(&text)
}

/// Copy the source slice covered by `span` into an owned `String`.
///
/// Lives next to `intern_span` because the source-map lock/unlock pattern
/// is identical — the reader guard must be dropped before the caller
/// mutates any other `Session` field (e.g. the interner, the handler).
fn span_text(session: &Session, span: Span) -> String {
    let sm = session.source_map.read().expect("source map poisoned");
    let file = sm.file(span.file);
    file.src[span.lo.0 as usize..span.hi.0 as usize].to_owned()
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
    fn reserved_word_ident_becomes_keyword_token() {
        // `int` is a reserved word (C99 §6.4.1); after phase-7 classification
        // it must surface as a `Keyword` token, not as `Ident`.
        let (mut sess, fid) = mk_session("int");
        let pp = tok(PpTokenKind::Ident, fid, 0, 3);
        let t = pp_to_token(&mut sess, pp).expect("keyword converts");
        assert_eq!(t.span, pp.span);
        assert_eq!(t.kind, TokenKind::Keyword(crate::keywords::Keyword::Int));
    }

    #[test]
    fn c99_underscore_keyword_becomes_keyword_token() {
        // Underscore-capital keywords are the C99 additions that typically
        // regress first if the map is built case-insensitively.
        let (mut sess, fid) = mk_session("_Bool");
        let pp = tok(PpTokenKind::Ident, fid, 0, 5);
        let t = pp_to_token(&mut sess, pp).expect("keyword converts");
        assert_eq!(t.kind, TokenKind::Keyword(crate::keywords::Keyword::Bool));
    }

    #[test]
    fn non_keyword_ident_stays_ident_after_classification() {
        let (mut sess, fid) = mk_session("printf");
        let pp = tok(PpTokenKind::Ident, fid, 0, 6);
        let t = pp_to_token(&mut sess, pp).expect("ident converts");
        match t.kind {
            TokenKind::Ident(sym) => assert_eq!(sess.interner.get(sym), "printf"),
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
    fn integer_pp_number_is_decoded_not_stubbed() {
        // Post-05-03 this must carry the real value + suffix, not the
        // placeholder zero that the earlier stub produced.
        let (mut sess, fid) = mk_session("42ULL");
        let pp = tok(PpTokenKind::PpNumber(PpNumberKind::Integer), fid, 0, 5);
        let t = pp_to_token(&mut sess, pp).expect("int converts");
        match t.kind {
            TokenKind::IntLit(lit) => {
                assert_eq!(lit.value, 42);
                assert_eq!(lit.suffix, IntSuffix::ULL);
            }
            other => panic!("expected IntLit, got {other:?}"),
        }
    }

    #[test]
    fn integer_overflow_emits_e0040_and_recovers_to_zero() {
        // A u128-overflowing literal must surface E0040 and still
        // produce an `IntLit` token so downstream invariants hold.
        let src = "0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF";
        let (mut sess, cap) = Session::for_test();
        let fid =
            sess.source_map.write().unwrap().add_file("t.c".into(), Arc::from(src.to_owned()));
        let pp = tok(PpTokenKind::PpNumber(PpNumberKind::Integer), fid, 0, src.len() as u32);
        let t = pp_to_token(&mut sess, pp).expect("int converts even on error");
        assert!(matches!(
            t.kind,
            TokenKind::IntLit(IntLiteral { value: 0, suffix: IntSuffix::None })
        ));
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some(rcc_errors::codes::E0040));
        // The diagnostic must carry the literal's span as a primary
        // label — that is the only way the user sees the offending text
        // underlined in rendered output.
        assert!(diags[0].labels.iter().any(|l| l.primary && l.span == pp.span));
    }

    #[test]
    fn float_pp_number_is_decoded_not_stubbed() {
        // Post-05-04 this must carry the real value + suffix, not the
        // placeholder 0.0 that the earlier stub produced.
        // 3.25 is exactly representable in binary, so an `==` match
        // is safe here; the task's own example `3.14` trips the
        // `clippy::approx_constant` lint (≈ π) in test code.
        let (mut sess, fid) = mk_session("3.25f");
        let pp = tok(PpTokenKind::PpNumber(PpNumberKind::Float), fid, 0, 5);
        let t = pp_to_token(&mut sess, pp).expect("float converts");
        match t.kind {
            TokenKind::FloatLit(lit) => {
                assert_eq!(lit.value, 3.25);
                assert_eq!(lit.suffix, FloatSuffix::F);
            }
            other => panic!("expected FloatLit, got {other:?}"),
        }
    }

    #[test]
    fn hex_float_pp_number_is_decoded_to_exact_value() {
        // `0x1.0p3` → 8.0 exactly (acceptance bullet for 05-04).
        let src = "0x1.0p3";
        let (mut sess, fid) = mk_session(src);
        let pp = tok(PpTokenKind::PpNumber(PpNumberKind::Float), fid, 0, src.len() as u32);
        let t = pp_to_token(&mut sess, pp).expect("hex float converts");
        match t.kind {
            TokenKind::FloatLit(lit) => {
                assert_eq!(lit.value, 8.0);
                assert_eq!(lit.suffix, FloatSuffix::None);
            }
            other => panic!("expected FloatLit, got {other:?}"),
        }
    }

    #[test]
    fn float_overflow_emits_w0002_with_infinity() {
        // `1e400` overflows double → the decoder returns +∞ and
        // phase-7 must attach a W0002 warning with the token span.
        let src = "1e400";
        let (mut sess, cap) = Session::for_test();
        let fid =
            sess.source_map.write().unwrap().add_file("t.c".into(), Arc::from(src.to_owned()));
        let pp = tok(PpTokenKind::PpNumber(PpNumberKind::Float), fid, 0, src.len() as u32);
        let t = pp_to_token(&mut sess, pp).expect("float converts even on overflow");
        match t.kind {
            TokenKind::FloatLit(lit) => {
                assert!(
                    lit.value.is_infinite() && lit.value > 0.0,
                    "expected +∞, got {}",
                    lit.value
                );
            }
            other => panic!("expected FloatLit, got {other:?}"),
        }
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some(rcc_errors::codes::W0002));
        assert_eq!(diags[0].level, rcc_errors::Level::Warning);
        assert!(diags[0].labels.iter().any(|l| l.primary && l.span == pp.span));
        // A warning must not bump the handler's error count.
        assert_eq!(sess.handler.error_count(), 0);
    }

    #[test]
    fn malformed_float_emits_error_and_recovers_to_zero() {
        // `1.0ff` — a double `f` suffix is not a legal float. The
        // decoder errors; phase-7 surfaces the diagnostic and keeps
        // a `FloatLit` placeholder so downstream invariants hold.
        let src = "1.0ff";
        let (mut sess, cap) = Session::for_test();
        let fid =
            sess.source_map.write().unwrap().add_file("t.c".into(), Arc::from(src.to_owned()));
        let pp = tok(PpTokenKind::PpNumber(PpNumberKind::Float), fid, 0, src.len() as u32);
        let t = pp_to_token(&mut sess, pp).expect("float converts even on error");
        assert!(matches!(
            t.kind,
            TokenKind::FloatLit(FloatLiteral { value: 0.0, suffix: FloatSuffix::None })
        ));
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].level, rcc_errors::Level::Error);
        assert!(diags[0].labels.iter().any(|l| l.primary && l.span == pp.span));
    }

    #[test]
    fn char_const_is_decoded_not_stubbed() {
        // Post-05-05 this must carry the real scalar value of the
        // decoded character, not the placeholder zero the earlier stub
        // produced. `L'a'` → 97 (§6.4.4.4p10 "single character" case).
        let (mut sess, fid) = mk_session("L'a'");
        let pp = tok(PpTokenKind::CharConst { enc: StringEncoding::Wide }, fid, 0, 4);
        let t = pp_to_token(&mut sess, pp).expect("char converts");
        match t.kind {
            TokenKind::CharLit(lit) => {
                assert_eq!(lit.value, 97);
                assert_eq!(lit.encoding, StringEncoding::Wide);
            }
            other => panic!("expected CharLit, got {other:?}"),
        }
    }

    #[test]
    fn multi_char_constant_emits_w0003() {
        // `'ab'` — §6.4.4.4p10 implementation-defined. Must warn with
        // W0003 and carry the span of the whole char-constant pp-token
        // as a primary label. Value packs the component bytes
        // big-endian so the warning is advisory, not destructive.
        let src = "'ab'";
        let (mut sess, cap) = Session::for_test();
        let fid =
            sess.source_map.write().unwrap().add_file("t.c".into(), Arc::from(src.to_owned()));
        let pp =
            tok(PpTokenKind::CharConst { enc: StringEncoding::None }, fid, 0, src.len() as u32);
        let t = pp_to_token(&mut sess, pp).expect("multi-char still converts");
        match t.kind {
            TokenKind::CharLit(lit) => {
                assert_eq!(lit.value, 0x6162, "expected big-endian packed bytes");
                assert_eq!(lit.encoding, StringEncoding::None);
            }
            other => panic!("expected CharLit, got {other:?}"),
        }
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some(rcc_errors::codes::W0003));
        assert_eq!(diags[0].level, rcc_errors::Level::Warning);
        assert!(diags[0].labels.iter().any(|l| l.primary && l.span == pp.span));
        // A warning must not bump the handler's error count.
        assert_eq!(sess.handler.error_count(), 0);
    }

    #[test]
    fn malformed_char_const_emits_error_and_recovers_to_zero() {
        // `'\Uxxxx'` — the lexer normally enforces the §6.4.3 UCN
        // shape, but if a malformed UCN ever slipped past we must
        // surface it as a diagnostic and still yield a placeholder
        // `CharLit` so downstream invariants hold.
        let src = "'\\Uxxxx'";
        let (mut sess, cap) = Session::for_test();
        let fid =
            sess.source_map.write().unwrap().add_file("t.c".into(), Arc::from(src.to_owned()));
        let pp =
            tok(PpTokenKind::CharConst { enc: StringEncoding::None }, fid, 0, src.len() as u32);
        let t = pp_to_token(&mut sess, pp).expect("char converts even on error");
        match t.kind {
            TokenKind::CharLit(lit) => {
                assert_eq!(lit.value, 0);
                assert_eq!(lit.encoding, StringEncoding::None);
            }
            other => panic!("expected CharLit, got {other:?}"),
        }
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].level, rcc_errors::Level::Error);
        assert!(diags[0].labels.iter().any(|l| l.primary && l.span == pp.span));
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
