//! Expression parsing (C99 §6.5).
//!
//! This task (05-07) only lands `parse_primary` — the leaves of the C99
//! expression grammar per §6.5.1:
//!
//! ```text
//! primary-expression:
//!     identifier
//!     constant            (integer / floating / character / enumeration)
//!     string-literal
//!     ( expression )
//! ```
//!
//! Postfix trailers (`a.b`, `a->b`, `a[i]`, `f(args)`, `++`, `--`,
//! compound literals) belong to task 05-09; binary / ternary / cast /
//! unary wiring belongs to 05-08..05-11.
//!
//! ## AST shape trade-off
//!
//! The current `ExprKind::IntLit { text: Symbol }` variants carry the
//! raw source spelling as an interned symbol rather than the already-
//! decoded `IntLiteral` / `FloatLiteral` / `CharLiteral` / `StringLiteral`
//! value that the phase-7 `Token` now holds. Two options were on the
//! table when wiring this task:
//!
//! 1. Keep the AST text-only and let typeck re-decode.
//! 2. Evolve the AST variants to carry the decoded payload.
//!
//! Option 2 is cleaner long-term (decoding happens once, errors are
//! attached at the single right span, typeck just reads fields), but it
//! reshapes five AST variants plus every downstream match — out of
//! scope for "primary expressions". This task takes **option 1**: the
//! parser re-interns the source slice behind the token's span and
//! stores the resulting `Symbol` in the AST, preserving decoded values
//! inside the `Token` stream that the postfix/unary tasks will thread
//! through. A follow-up task (see `## TODO` below) will migrate the
//! AST to the decoded shape once the broader expression grammar is in.
//!
//! ## Parenthesised-expression stub
//!
//! `( expression )` requires a full expression parser, which does not
//! exist yet — `parse_expression` lands in tasks 05-08 (Pratt) and
//! 05-12 (comma). Until then, this module recursively calls
//! `parse_primary` for the body so the acceptance case `(42)` — and
//! the nested `(((42)))` — can round-trip now without pulling the rest
//! of the expression grammar forward. The error recovery is simple:
//! on a missing inner primary the outer `Paren` arm returns `None`;
//! on a missing `)` it still returns the inner expression unwrapped
//! (not wrapped in `Paren`) and diagnoses the unbalanced paren so the
//! rest of the token stream is not desynchronised. Once task 05-12
//! lands, the inner call becomes `parse_expression` and this module's
//! TODO goes away.
//!
//! ## TODO
//!
//! - [ ] 05-12 or later: swap the recursive `parse_primary` stub for
//!   the real top-level expression entry point.
//! - [ ] post-M1: migrate `ExprKind::{Int,Float,Char,String}Lit` to
//!   carry decoded payloads (`IntLiteral`, `FloatLiteral`,
//!   `CharLiteral`, `StringLiteral`) instead of `text: Symbol`.

use rcc_ast::{Expr, ExprKind};
use rcc_lexer::Punct;
use rcc_span::Symbol;

use crate::token::TokenKind;
use crate::Parser;

/// Parse a C99 §6.5.1 *primary-expression*.
///
/// Returns `None` when the current token cannot start a primary
/// expression (including end-of-input). The caller receives a `None`
/// only **after** a diagnostic has been emitted through the session's
/// handler; the token stream is left on the offending token so higher-
/// level callers can decide whether to skip-to-recovery or propagate.
///
/// The `( expression )` arm is currently stubbed: the inner production
/// recursively delegates to `parse_primary`, not to the full
/// expression parser (which doesn't exist yet). See the module-level
/// docs for the rationale.
pub fn parse_primary(p: &mut Parser<'_>) -> Option<Expr> {
    // `peek().cloned()?` keeps the cursor on the current token (so the
    // error arm below can still read its span) while letting us match
    // on the owned `TokenKind` without fighting the borrow checker.
    let tok = p.peek().cloned()?;
    let span = tok.span;
    match tok.kind {
        TokenKind::Ident(sym) => {
            p.bump();
            let id = p.fresh_id();
            Some(Expr { id, kind: ExprKind::Ident(sym), span })
        }
        TokenKind::IntLit(_) => {
            // AST is still text-based for literals; re-intern the source
            // slice covered by the token. The decoded `IntLiteral`
            // value stays inside the `Token` and will be threaded to
            // typeck once the AST evolves.
            let sym = intern_span_text(p, span);
            p.bump();
            let id = p.fresh_id();
            Some(Expr { id, kind: ExprKind::IntLit { text: sym }, span })
        }
        TokenKind::FloatLit(_) => {
            let sym = intern_span_text(p, span);
            p.bump();
            let id = p.fresh_id();
            Some(Expr { id, kind: ExprKind::FloatLit { text: sym }, span })
        }
        TokenKind::CharLit(_) => {
            let sym = intern_span_text(p, span);
            p.bump();
            let id = p.fresh_id();
            Some(Expr { id, kind: ExprKind::CharLit { text: sym }, span })
        }
        TokenKind::StringLit(_) => {
            let sym = intern_span_text(p, span);
            p.bump();
            let id = p.fresh_id();
            Some(Expr { id, kind: ExprKind::StringLit { text: sym }, span })
        }
        TokenKind::Punct(Punct::LParen) => {
            let lparen_span = span;
            p.bump();
            // Inner production is stubbed — see module docs.
            let inner = parse_primary(p)?;
            // Consume the closing `)` if present; otherwise diagnose
            // and return the inner expression unwrapped so the caller
            // can keep making progress on the remaining stream.
            match p.peek() {
                Some(t) if matches!(t.kind, TokenKind::Punct(Punct::RParen)) => {
                    let rparen_span = t.span;
                    p.bump();
                    let full_span = lparen_span.to(rparen_span);
                    let id = p.fresh_id();
                    Some(Expr { id, kind: ExprKind::Paren(Box::new(inner)), span: full_span })
                }
                _ => {
                    let at = p.cur_span();
                    p.session
                        .handler
                        .struct_err(at, "expected `)` to close parenthesised expression")
                        .label(lparen_span, "unmatched `(` here")
                        .emit();
                    Some(inner)
                }
            }
        }
        _ => {
            p.session.handler.struct_err(span, "expected primary expression").emit();
            None
        }
    }
}

/// Intern the source text covered by `span` through the session's
/// interner and return the resulting `Symbol`. This is the classic
/// "read + intern" pattern used in phase-7 conversion; lifted here so
/// every literal arm stays a one-liner.
///
/// The source-map read guard is explicitly dropped before we touch the
/// interner because `Session::interner` is a sibling field to
/// `source_map` and holding the guard while mutably borrowing a second
/// field would deadlock on any concurrent writer — the same idiom that
/// `phase7::intern_span` follows.
fn intern_span_text(p: &mut Parser<'_>, span: rcc_span::Span) -> Symbol {
    let text = {
        let sm = p.session.source_map.read().expect("source map poisoned");
        let file = sm.file(span.file);
        file.src[span.lo.0 as usize..span.hi.0 as usize].to_owned()
    };
    p.session.interner.intern(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phase7::convert;
    use rcc_lexer::{PpNumberKind, PpToken, PpTokenKind, Punct, StringEncoding};
    use rcc_session::Session;
    use rcc_span::{BytePos, FileId, Span};
    use std::sync::Arc;

    // ── Test scaffolding ────────────────────────────────────────────

    /// Build a `(Session, FileId)` pair backed by `src`.
    fn mk_session(src: &str) -> (Session, FileId, rcc_errors::CaptureEmitter) {
        let (sess, cap) = Session::for_test();
        let fid =
            sess.source_map.write().unwrap().add_file("t.c".into(), Arc::from(src.to_owned()));
        (sess, fid, cap)
    }

    /// One pp-token with span `lo..hi` in `file`.
    fn pp(kind: PpTokenKind, file: FileId, lo: u32, hi: u32) -> PpToken {
        PpToken {
            kind,
            span: Span::new(file, BytePos(lo), BytePos(hi)),
            leading_ws: false,
            at_line_start: false,
        }
    }

    // ── Each arm of the primary-expression grammar ──────────────────

    #[test]
    fn integer_literal_parses_to_intlit() {
        // `42` → ExprKind::IntLit { text: "42" }, span covers the digits,
        // and the decoded value is still reachable through the token.
        let src = "42";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = [pp(PpTokenKind::PpNumber(PpNumberKind::Integer), fid, 0, 2)];
        let tokens = convert(&mut sess, &pps);
        // Decoded value must be on the token stream (this is the
        // "reachable" side of the acceptance: the parser preserves the
        // raw text, but the post-phase-7 token carries the u128 value).
        match &tokens[0].kind {
            TokenKind::IntLit(lit) => assert_eq!(lit.value, 42),
            other => panic!("expected IntLit, got {other:?}"),
        }
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_primary(&mut parser).expect("42 parses");
        match e.kind {
            ExprKind::IntLit { text } => {
                assert_eq!(parser.session.interner.get(text), "42");
            }
            other => panic!("expected IntLit, got {other:?}"),
        }
        assert_eq!(e.span.lo.0, 0);
        assert_eq!(e.span.hi.0, 2);
        // Cursor advanced past the consumed token.
        assert_eq!(parser.cursor, 1);
    }

    #[test]
    fn identifier_parses_to_ident_even_when_unknown() {
        // §6.5.1 primary: `foo` → ExprKind::Ident(sym). Name
        // resolution is HIR-lowering's job, so an undeclared name must
        // still parse cleanly.
        let src = "foo";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = [pp(PpTokenKind::Ident, fid, 0, 3)];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_primary(&mut parser).expect("foo parses");
        match e.kind {
            ExprKind::Ident(sym) => {
                assert_eq!(parser.session.interner.get(sym), "foo");
            }
            other => panic!("expected Ident, got {other:?}"),
        }
        assert!(cap.diagnostics().is_empty(), "no diagnostics for bare ident");
    }

    #[test]
    fn string_literal_parses_to_stringlit() {
        // `"hi"` → ExprKind::StringLit.
        let src = "\"hi\"";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = [pp(PpTokenKind::StringLit { enc: StringEncoding::None }, fid, 0, 4)];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_primary(&mut parser).expect(r#""hi" parses"#);
        match e.kind {
            ExprKind::StringLit { text } => {
                assert_eq!(parser.session.interner.get(text), "\"hi\"");
            }
            other => panic!("expected StringLit, got {other:?}"),
        }
    }

    #[test]
    fn char_literal_parses_to_charlit() {
        // `'a'` → ExprKind::CharLit.
        let src = "'a'";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = [pp(PpTokenKind::CharConst { enc: StringEncoding::None }, fid, 0, 3)];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_primary(&mut parser).expect("'a' parses");
        match e.kind {
            ExprKind::CharLit { text } => {
                assert_eq!(parser.session.interner.get(text), "'a'");
            }
            other => panic!("expected CharLit, got {other:?}"),
        }
    }

    #[test]
    fn float_literal_parses_to_floatlit() {
        // `3.14` → ExprKind::FloatLit.
        let src = "3.14";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = [pp(PpTokenKind::PpNumber(PpNumberKind::Float), fid, 0, 4)];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_primary(&mut parser).expect("3.14 parses");
        match e.kind {
            ExprKind::FloatLit { text } => {
                assert_eq!(parser.session.interner.get(text), "3.14");
            }
            other => panic!("expected FloatLit, got {other:?}"),
        }
    }

    // ── Parenthesised expressions ───────────────────────────────────

    #[test]
    fn paren_wraps_inner_primary() {
        // Acceptance bullet: parsing `(42)` yields
        // `ExprKind::Paren(Expr::IntLit {..})` with the span covering
        // the whole `(42)` run.
        let src = "(42)";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = [
            pp(PpTokenKind::Punct(Punct::LParen), fid, 0, 1),
            pp(PpTokenKind::PpNumber(PpNumberKind::Integer), fid, 1, 3),
            pp(PpTokenKind::Punct(Punct::RParen), fid, 3, 4),
        ];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_primary(&mut parser).expect("(42) parses");
        match e.kind {
            ExprKind::Paren(inner) => match inner.kind {
                ExprKind::IntLit { text } => {
                    assert_eq!(parser.session.interner.get(text), "42");
                }
                other => panic!("inner must be IntLit, got {other:?}"),
            },
            other => panic!("expected Paren, got {other:?}"),
        }
        assert_eq!(e.span.lo.0, 0);
        assert_eq!(e.span.hi.0, 4);
        // Cursor consumed `(`, `42`, and `)` — three tokens.
        assert_eq!(parser.cursor, 3);
    }

    #[test]
    fn nested_parens_stack() {
        // `(((42)))` nests three `Paren` wrappers — verifies the
        // recursive stub handles arbitrary depth cleanly.
        let src = "(((42)))";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = [
            pp(PpTokenKind::Punct(Punct::LParen), fid, 0, 1),
            pp(PpTokenKind::Punct(Punct::LParen), fid, 1, 2),
            pp(PpTokenKind::Punct(Punct::LParen), fid, 2, 3),
            pp(PpTokenKind::PpNumber(PpNumberKind::Integer), fid, 3, 5),
            pp(PpTokenKind::Punct(Punct::RParen), fid, 5, 6),
            pp(PpTokenKind::Punct(Punct::RParen), fid, 6, 7),
            pp(PpTokenKind::Punct(Punct::RParen), fid, 7, 8),
        ];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_primary(&mut parser).expect("(((42))) parses");
        // Walk three levels of Paren and land on IntLit.
        let l1 = match e.kind {
            ExprKind::Paren(inner) => *inner,
            other => panic!("level 1 must be Paren, got {other:?}"),
        };
        let l2 = match l1.kind {
            ExprKind::Paren(inner) => *inner,
            other => panic!("level 2 must be Paren, got {other:?}"),
        };
        let l3 = match l2.kind {
            ExprKind::Paren(inner) => *inner,
            other => panic!("level 3 must be Paren, got {other:?}"),
        };
        assert!(matches!(l3.kind, ExprKind::IntLit { .. }));
        // Span of the outermost Paren must cover the whole source.
        assert_eq!(e.span.lo.0, 0);
        assert_eq!(e.span.hi.0, 8);
    }

    // ── Error paths ─────────────────────────────────────────────────

    #[test]
    fn empty_input_returns_none_without_diagnostic() {
        // End-of-input is a legitimate "nothing to parse" — not an
        // error by itself; the caller decides whether absence of a
        // primary at a given position is a problem. (A statement-
        // expression parser would issue its own "expected expression"
        // diagnostic with a more specific span.)
        let (mut sess, _fid, cap) = mk_session("");
        let mut parser = Parser::new(&mut sess, Vec::new());
        let e = parse_primary(&mut parser);
        assert!(e.is_none());
        assert!(cap.diagnostics().is_empty(), "empty input is quiet");
    }

    #[test]
    fn unexpected_punct_emits_diagnostic_and_returns_none() {
        // `+` at the head of a primary-expression is a syntax error:
        // the grammar has no leading `+` here (unary `+` is parsed
        // one level up, in 05-09 unary).
        let src = "+";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = [pp(PpTokenKind::Punct(Punct::Plus), fid, 0, 1)];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_primary(&mut parser);
        assert!(e.is_none());
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("primary expression"),
            "message should mention primary expression, got {:?}",
            diags[0].message,
        );
        // Primary label must cover the offending token so the
        // rendered error underlines the right character.
        assert!(
            diags[0].labels.iter().any(|l| l.primary && l.span.lo.0 == 0 && l.span.hi.0 == 1),
            "primary label must point at the `+`",
        );
    }

    #[test]
    fn unclosed_paren_emits_diagnostic_and_returns_inner() {
        // `(42` — missing `)`. Recovery: return the inner `42` so
        // higher-level callers make progress, and diagnose the
        // unmatched `(` with both primary (at end-of-input) and
        // secondary (at the unmatched `(`) labels.
        let src = "(42";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = [
            pp(PpTokenKind::Punct(Punct::LParen), fid, 0, 1),
            pp(PpTokenKind::PpNumber(PpNumberKind::Integer), fid, 1, 3),
        ];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_primary(&mut parser).expect("recovery returns the inner expr");
        assert!(matches!(e.kind, ExprKind::IntLit { .. }));
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("expected `)`"));
    }

    #[test]
    fn fresh_id_is_monotonic_and_unique() {
        // Two consecutive primaries must receive two distinct NodeIds
        // — this is the invariant downstream (`rcc_hir_lower`) relies
        // on to build side tables keyed by NodeId.
        let src = "a b";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = [
            pp(PpTokenKind::Ident, fid, 0, 1),
            pp(PpTokenKind::Whitespace, fid, 1, 2),
            pp(PpTokenKind::Ident, fid, 2, 3),
        ];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e1 = parse_primary(&mut parser).expect("a parses");
        let e2 = parse_primary(&mut parser).expect("b parses");
        assert_ne!(e1.id, e2.id, "NodeIds must be unique per node");
        assert!(e1.id.0 < e2.id.0, "NodeIds must be monotonically increasing");
    }
}
