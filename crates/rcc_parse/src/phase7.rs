//! C99 §5.1.1.2 phase 7: convert a preprocessed pp-token stream into the
//! parser-level [`Token`] type.
//!
//! This module owns keyword classification, literal decoding dispatch,
//! and adjacent string concatenation for the token stream consumed by
//! the parser. The match arms are intentionally exhaustive so adding a
//! new [`PpTokenKind`] variant produces a compile error until it is
//! handled here.
//!
//! Whitespace, newlines, the lexer's EOF sentinel, and [`PpTokenKind::Unknown`]
//! are intentionally dropped — the parser consumes a stream of *parser*
//! tokens only, and lexical categories that do not survive phase 7 have no
//! representation in [`TokenKind`].

use rcc_errors::{codes, Diagnostic, Label, Level};
use rcc_lexer::{strip_line_splices, PpNumberKind, PpToken, PpTokenKind};
use rcc_session::Session;
use rcc_span::Span;

use rcc_lexer::StringEncoding;

use crate::keywords::classify_ident;
use crate::literal::{decode_char_full, decode_float, decode_integer_with_options, decode_string};
use crate::token::{
    CharLiteral, FloatLiteral, FloatSuffix, IntBase, IntLiteral, IntSuffix, StringLiteral, Token,
    TokenKind,
};

/// Convert a slice of pp-tokens into a fresh `Vec<Token>`.
///
/// Whitespace, newlines, the lexer's EOF sentinel, and any `Unknown`
/// tokens (already diagnosed by the lexer) are silently dropped.
/// Adjacent string-literal concatenation is performed by
/// [`merge_adjacent_strings`] after per-literal decoding.
pub fn convert(session: &mut Session, pp: &[PpToken]) -> Vec<Token> {
    let mut out = Vec::with_capacity(pp.len());
    for tok in pp {
        if let Some(t) = pp_to_token(session, *tok) {
            out.push(t);
        }
    }
    // C99 §6.4.5p5 adjacent-string concatenation must run after
    // per-literal decoding so the merge pass sees fully decoded byte
    // payloads rather than raw pp-token slices. Hoisting it into
    // `convert` (rather than leaving it to callers) keeps the phase-7
    // contract simple: one call, one fully-converted token stream.
    merge_adjacent_strings(session, out)
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
            // keyword match runs against the canonical text (C99/C11 §6.4.1).
            // C keywords are real reserved words at every position in the
            // grammar — there is no context-sensitive keyword list like C++'s
            // `override` / `final`, so a one-shot table lookup suffices.
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
            match decode_integer_with_options(&text, session.opts.gnu_binary_integer_literals) {
                Ok(lit) => TokenKind::IntLit(lit),
                Err(mut diag) => {
                    diag.labels.push(Label {
                        span: pp.span,
                        message: String::new(),
                        primary: true,
                    });
                    session.handler.emit(&diag);
                    TokenKind::IntLit(IntLiteral {
                        value: 0,
                        base: IntBase::Decimal,
                        suffix: IntSuffix::None,
                    })
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
            // C99 §6.4.5 string-literal decoding. `decode_string` returns a
            // spanless `Diagnostic` on escape errors; we attach the
            // pp-token's own span here so the user sees the offending
            // spelling underlined. On error we still yield a `StringLit`
            // token carrying an empty byte payload so downstream
            // invariants ("every pp-string becomes a StringLit") hold
            // through recovery. Adjacent-literal concatenation runs as a
            // separate pass (`merge_adjacent_strings`) on top of this
            // per-token decoding; see `convert`.
            let text = span_text(session, pp.span);
            match decode_string(&text, enc) {
                Ok(bytes) => TokenKind::StringLit(StringLiteral { bytes, encoding: enc }),
                Err(mut diag) => {
                    diag.labels.push(Label {
                        span: pp.span,
                        message: String::new(),
                        primary: true,
                    });
                    session.handler.emit(&diag);
                    TokenKind::StringLit(StringLiteral { bytes: Vec::new(), encoding: enc })
                }
            }
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

/// Concatenate adjacent `StringLit` tokens per C99 §6.4.5p5 (translation
/// phase 6).
///
/// Runs over an already-decoded token stream and collapses every
/// maximal run of consecutive `StringLit` tokens into a single
/// `StringLit`:
///
/// - The merged `bytes` payload is the concatenation of the
///   contributing payloads in source order (no separator, no trailing
///   NUL — typeck appends `\0` when the literal is used as an array
///   initializer per §6.4.5p6).
/// - The merged `encoding` follows [`promote_encoding`]: identical
///   prefixes keep the prefix; a narrow (unprefixed) literal and an
///   `L`-prefixed wide literal promote to `Wide` (either order —
///   §6.4.5p5 "if any of the tokens are wide, the result is wide");
///   every other distinct-prefix mix is a constraint violation
///   ([`codes::E0041`]).
/// - The merged `span` covers every contributing literal via
///   [`Span::to`] so downstream diagnostics can underline the whole
///   run rather than just the head token.
///
/// On encoding conflict the offending (later) literal receives an
/// E0041 primary label and the first literal of the current run gets
/// a secondary "previous string literal here" label. The run is then
/// closed at the offending token; the offender itself seeds a fresh
/// run on the next outer-loop iteration so later well-formed
/// concatenations still work (error recovery).
///
/// Non-string tokens are passed through unchanged. The input vector
/// is consumed and a fresh `Vec<Token>` is returned — cheaper than
/// an in-place mutate that would have to shift tail elements on every
/// merged run.
pub fn merge_adjacent_strings(session: &mut Session, tokens: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i].kind {
            TokenKind::StringLit(head) => {
                let start_span = tokens[i].span;
                let mut bytes: Vec<u8> = head.bytes.clone();
                let mut enc: StringEncoding = head.encoding;
                let mut span = start_span;
                let mut j = i + 1;
                while j < tokens.len() {
                    let next = match &tokens[j].kind {
                        TokenKind::StringLit(s) => s,
                        _ => break,
                    };
                    match promote_encoding(enc, next.encoding) {
                        Some(new_enc) => {
                            enc = new_enc;
                            bytes.extend_from_slice(&next.bytes);
                            // Only merge spans if both are in the same file;
                            // after preprocessing, adjacent strings may come
                            // from different files (e.g. via #include).
                            if span.file == tokens[j].span.file {
                                span = span.to(tokens[j].span);
                            }
                            j += 1;
                        }
                        None => {
                            session
                                .handler
                                .struct_err(tokens[j].span, "incompatible string literal encodings")
                                .code(codes::E0041)
                                .label(start_span, "previous string literal here")
                                .emit();
                            // Close the current run here; the outer
                            // loop restarts at `j` so the offending
                            // token begins a new run (allowing any
                            // subsequent well-formed concatenation to
                            // still succeed).
                            break;
                        }
                    }
                }
                out.push(Token {
                    kind: TokenKind::StringLit(StringLiteral { bytes, encoding: enc }),
                    span,
                });
                i = j;
            }
            _ => {
                out.push(tokens[i].clone());
                i += 1;
            }
        }
    }
    out
}

/// Compute the encoding of the concatenation of two string-literal
/// tokens per C99 §6.4.5p5.
///
/// Returns `None` when the combination is ill-formed (a constraint
/// violation triggering [`codes::E0041`]). The rule is:
///
/// - Identical prefixes (`None+None`, `L+L`, `u+u`, `U+U`, `u8+u8`)
///   pass through unchanged.
/// - `None` (narrow) and `Wide` (`L`) combine, in either order, to
///   `Wide` — the only promotion C99 §6.4.5p5 specifies ("if any of
///   the tokens are wide string literal tokens, the resulting
///   sequence is treated as a wide string literal").
/// - Every other mix (`None+Utf16`, `Wide+Utf32`, `Utf16+Utf32`, …)
///   is undefined behavior in C99 and rejected here.
fn promote_encoding(a: StringEncoding, b: StringEncoding) -> Option<StringEncoding> {
    if a == b {
        return Some(a);
    }
    match (a, b) {
        (StringEncoding::None, StringEncoding::Wide)
        | (StringEncoding::Wide, StringEncoding::None) => Some(StringEncoding::Wide),
        _ => None,
    }
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
    strip_line_splices(&file.src[span.lo.0 as usize..span.hi.0 as usize])
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcc_errors::Handler;
    use rcc_lexer::{Punct, StringEncoding};
    use rcc_session::{Options, Session};
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
    fn spliced_ident_uses_logical_spelling_for_interning() {
        let src = "fo\\\r\no";
        let (mut sess, fid) = mk_session(src);
        let pp = tok(PpTokenKind::Ident, fid, 0, src.len() as u32);
        let t = pp_to_token(&mut sess, pp).expect("ident converts");
        assert_eq!(t.span, pp.span, "physical span remains diagnostic-accurate");
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
    fn c11_underscore_keywords_become_keyword_tokens_in_c11_mode() {
        for (spelling, kw) in [
            ("_Alignas", crate::keywords::Keyword::Alignas),
            ("_Alignof", crate::keywords::Keyword::Alignof),
            ("_Atomic", crate::keywords::Keyword::Atomic),
            ("_Generic", crate::keywords::Keyword::Generic),
            ("_Noreturn", crate::keywords::Keyword::Noreturn),
            ("_Static_assert", crate::keywords::Keyword::StaticAssert),
            ("_Thread_local", crate::keywords::Keyword::ThreadLocal),
        ] {
            let opts = Options {
                language_standard: rcc_session::LanguageStandard::C11,
                ..Options::default()
            };
            let sess = Session::new(opts);
            let fid = sess
                .source_map
                .write()
                .unwrap()
                .add_file("t.c".into(), Arc::from(spelling.to_owned()));
            let mut sess = sess;
            let pp = tok(PpTokenKind::Ident, fid, 0, spelling.len() as u32);
            let t = pp_to_token(&mut sess, pp).expect("keyword converts");
            assert_eq!(t.kind, TokenKind::Keyword(kw), "{spelling}");
        }
    }

    #[test]
    fn c11_keywords_are_reserved_in_c99_mode_too() {
        // rcc's C99 policy is explicit: these implementation-reserved
        // `_`+uppercase spellings are classified as future keywords rather
        // than accepted as ordinary identifiers.
        for (spelling, kw) in [
            ("_Alignas", crate::keywords::Keyword::Alignas),
            ("_Alignof", crate::keywords::Keyword::Alignof),
            ("_Atomic", crate::keywords::Keyword::Atomic),
            ("_Generic", crate::keywords::Keyword::Generic),
            ("_Noreturn", crate::keywords::Keyword::Noreturn),
            ("_Static_assert", crate::keywords::Keyword::StaticAssert),
            ("_Thread_local", crate::keywords::Keyword::ThreadLocal),
        ] {
            let (mut sess, fid) = mk_session(spelling);
            let pp = tok(PpTokenKind::Ident, fid, 0, spelling.len() as u32);
            let t = pp_to_token(&mut sess, pp).expect("keyword converts");
            assert_eq!(t.kind, TokenKind::Keyword(kw), "{spelling}");
        }
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
                assert_eq!(lit.base, IntBase::Decimal);
                assert_eq!(lit.suffix, IntSuffix::ULL);
            }
            other => panic!("expected IntLit, got {other:?}"),
        }
    }

    #[test]
    fn binary_integer_pp_number_respects_gnu_option() {
        let src = "0b10011";
        let cap = rcc_errors::CaptureEmitter::new();
        let handler = Handler::with_emitter(Box::new(cap.clone()));
        let mut sess = Session::with_handler(
            Options { gnu_binary_integer_literals: true, ..Options::default() },
            handler,
        );
        let fid =
            sess.source_map.write().unwrap().add_file("t.c".into(), Arc::from(src.to_owned()));
        let pp = tok(PpTokenKind::PpNumber(PpNumberKind::Integer), fid, 0, src.len() as u32);
        let t = pp_to_token(&mut sess, pp).expect("binary int converts");
        match t.kind {
            TokenKind::IntLit(lit) => {
                assert_eq!(lit.value, 19);
                assert_eq!(lit.base, IntBase::Binary);
                assert_eq!(lit.suffix, IntSuffix::None);
            }
            other => panic!("expected IntLit, got {other:?}"),
        }
        assert!(cap.diagnostics().is_empty());
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
            TokenKind::IntLit(IntLiteral {
                value: 0,
                base: IntBase::Decimal,
                suffix: IntSuffix::None,
            })
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
    fn string_lit_is_decoded_not_stubbed() {
        // Post-05-06 this must carry the decoded byte payload, not the
        // earlier empty-bytes stub. `u8"hi"` → b"hi", encoding Utf8.
        // Note: the prefix for `u8` is two bytes; the pp-token span
        // covers the whole source slice including it.
        let (mut sess, fid) = mk_session("u8\"hi\"");
        let pp = tok(PpTokenKind::StringLit { enc: StringEncoding::Utf8 }, fid, 0, 6);
        let t = pp_to_token(&mut sess, pp).expect("string converts");
        match t.kind {
            TokenKind::StringLit(lit) => {
                assert_eq!(lit.bytes, b"hi".to_vec(), "bytes decoded without trailing NUL");
                assert_eq!(lit.encoding, StringEncoding::Utf8);
            }
            other => panic!("expected StringLit, got {other:?}"),
        }
    }

    #[test]
    fn narrow_string_decodes_with_hex_escape() {
        // Task acceptance bullet: `"\xff"` → byte 255, encoding None.
        let src = "\"\\xff\"";
        let (mut sess, fid) = mk_session(src);
        let pp =
            tok(PpTokenKind::StringLit { enc: StringEncoding::None }, fid, 0, src.len() as u32);
        let t = pp_to_token(&mut sess, pp).expect("string converts");
        match t.kind {
            TokenKind::StringLit(lit) => {
                assert_eq!(lit.bytes, vec![0xff]);
                assert_eq!(lit.encoding, StringEncoding::None);
            }
            other => panic!("expected StringLit, got {other:?}"),
        }
    }

    #[test]
    fn malformed_string_lit_emits_error_and_recovers_to_empty_bytes() {
        // Unknown simple escape inside a string. The decoder errors,
        // phase-7 attaches the token's span, and we still yield a
        // placeholder `StringLit` so downstream invariants hold.
        let src = "\"\\q\"";
        let (mut sess, cap) = Session::for_test();
        let fid =
            sess.source_map.write().unwrap().add_file("t.c".into(), Arc::from(src.to_owned()));
        let pp =
            tok(PpTokenKind::StringLit { enc: StringEncoding::None }, fid, 0, src.len() as u32);
        let t = pp_to_token(&mut sess, pp).expect("string converts even on error");
        match t.kind {
            TokenKind::StringLit(lit) => {
                assert!(lit.bytes.is_empty());
                assert_eq!(lit.encoding, StringEncoding::None);
            }
            other => panic!("expected StringLit, got {other:?}"),
        }
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some(rcc_errors::codes::E0005));
        assert!(diags[0].labels.iter().any(|l| l.primary && l.span == pp.span));
    }

    // ── merge_adjacent_strings (C99 §6.4.5p5) ───────────────────────

    /// Build adjacent string-literal pp-tokens over `src` with the
    /// given `(lo, hi, enc)` ranges and run `convert` (which includes
    /// the merge pass). Returns the converted stream plus the session
    /// and capture handle so the caller can inspect error counts and
    /// diagnostics.
    ///
    /// Helper kept local to the merge tests — the wider phase-7 tests
    /// use `mk_session` + raw `pp_to_token` calls, but the merge suite
    /// needs the `convert` entry point so the pass actually runs.
    fn make_and_convert(
        src: &str,
        ranges: &[(u32, u32, StringEncoding)],
    ) -> (Vec<Token>, Session, rcc_errors::CaptureEmitter) {
        let (mut sess, cap) = Session::for_test();
        let fid =
            sess.source_map.write().unwrap().add_file("t.c".into(), Arc::from(src.to_owned()));
        let stream: Vec<PpToken> = ranges
            .iter()
            .map(|&(lo, hi, enc)| tok(PpTokenKind::StringLit { enc }, fid, lo, hi))
            .collect();
        let out = convert(&mut sess, &stream);
        (out, sess, cap)
    }

    #[test]
    fn adjacent_narrow_strings_concat_and_keep_narrow_encoding() {
        // Acceptance bullet: `"a" "b" "c"` → bytes `[97, 98, 99]`,
        // encoding None. Source layout: `"a""b""c"` at byte offsets
        // 0..3, 3..6, 6..9 (each `"x"` is three bytes).
        let src = r#""a""b""c""#;
        let (out, _sess, _cap) = make_and_convert(
            src,
            &[
                (0, 3, StringEncoding::None),
                (3, 6, StringEncoding::None),
                (6, 9, StringEncoding::None),
            ],
        );
        assert_eq!(out.len(), 1, "three adjacent literals collapse to one");
        match &out[0].kind {
            TokenKind::StringLit(lit) => {
                assert_eq!(lit.bytes, vec![b'a', b'b', b'c']);
                assert_eq!(lit.encoding, StringEncoding::None);
            }
            other => panic!("expected StringLit, got {other:?}"),
        }
        // Span must cover the whole run (§6.4.5p5 semantics).
        assert_eq!(out[0].span.lo.0, 0);
        assert_eq!(out[0].span.hi.0, 9);
    }

    #[test]
    fn narrow_then_wide_promotes_to_wide() {
        // `"a" L"b"` — narrow followed by wide; §6.4.5p5 promotes the
        // result to wide.
        let src = r#""a"L"b""#;
        let (out, _sess, _cap) =
            make_and_convert(src, &[(0, 3, StringEncoding::None), (3, 7, StringEncoding::Wide)]);
        assert_eq!(out.len(), 1);
        match &out[0].kind {
            TokenKind::StringLit(lit) => {
                assert_eq!(lit.bytes, vec![b'a', b'b']);
                assert_eq!(lit.encoding, StringEncoding::Wide);
            }
            other => panic!("expected StringLit, got {other:?}"),
        }
    }

    #[test]
    fn wide_then_narrow_promotes_to_wide() {
        // `L"a" "b"` — wide followed by narrow; the result is still
        // wide (the "if any of the tokens are wide" clause).
        let src = r#"L"a""b""#;
        let (out, _sess, _cap) =
            make_and_convert(src, &[(0, 4, StringEncoding::Wide), (4, 7, StringEncoding::None)]);
        assert_eq!(out.len(), 1);
        match &out[0].kind {
            TokenKind::StringLit(lit) => {
                assert_eq!(lit.bytes, vec![b'a', b'b']);
                assert_eq!(lit.encoding, StringEncoding::Wide);
            }
            other => panic!("expected StringLit, got {other:?}"),
        }
    }

    #[test]
    fn wide_plus_utf32_emits_e0041() {
        // `L"a" U"b"` — a genuinely incompatible mix (wide + UTF-32).
        // §6.4.5p5 promotes only narrow + wide; everything else is a
        // constraint violation (E0041).
        let src = r#"L"a"U"b""#;
        let (out, sess, cap) =
            make_and_convert(src, &[(0, 4, StringEncoding::Wide), (4, 8, StringEncoding::Utf32)]);
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, Some(rcc_errors::codes::E0041));
        assert_eq!(sess.handler.error_count(), 1);
        // Error recovery: we emit two separate StringLits rather than
        // destructively merging an ill-formed run.
        assert_eq!(out.len(), 2, "error recovery splits at the conflict");
    }

    #[test]
    fn same_utf16_prefix_allows_concat() {
        // `u"A" u"B"` — identical UTF-16 prefixes concatenate cleanly.
        let src = r#"u"A"u"B""#;
        let (out, _sess, cap) =
            make_and_convert(src, &[(0, 4, StringEncoding::Utf16), (4, 8, StringEncoding::Utf16)]);
        assert!(cap.diagnostics().is_empty(), "same-prefix concat must not warn");
        assert_eq!(out.len(), 1);
        match &out[0].kind {
            TokenKind::StringLit(lit) => {
                assert_eq!(lit.bytes, vec![b'A', b'B']);
                assert_eq!(lit.encoding, StringEncoding::Utf16);
            }
            other => panic!("expected StringLit, got {other:?}"),
        }
    }

    #[test]
    fn non_string_tokens_pass_through_unchanged() {
        // An identifier sandwiched between two strings must NOT be
        // swallowed by the merge pass. Layout: `"a" foo "b"`.
        let src = "\"a\" foo \"b\"";
        let (mut sess, _cap) = Session::for_test();
        let fid =
            sess.source_map.write().unwrap().add_file("t.c".into(), Arc::from(src.to_owned()));
        let stream = [
            tok(PpTokenKind::StringLit { enc: StringEncoding::None }, fid, 0, 3),
            tok(PpTokenKind::Whitespace, fid, 3, 4),
            tok(PpTokenKind::Ident, fid, 4, 7),
            tok(PpTokenKind::Whitespace, fid, 7, 8),
            tok(PpTokenKind::StringLit { enc: StringEncoding::None }, fid, 8, 11),
        ];
        let out = convert(&mut sess, &stream);
        assert_eq!(out.len(), 3);
        assert!(matches!(out[0].kind, TokenKind::StringLit(_)));
        assert!(matches!(out[1].kind, TokenKind::Ident(_)));
        assert!(matches!(out[2].kind, TokenKind::StringLit(_)));
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
