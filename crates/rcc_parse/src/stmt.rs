//! Statement parsing (C99 §6.8).
//!
//! This task (05-13) lands the three simplest statement shapes plus
//! the block / block-item machinery they hang off:
//!
//! - *expression-statement* (§6.8.3)  — `expr ;`
//! - *null statement*        (§6.8.3) — a lone `;`
//! - *compound statement*    (§6.8.2) — `{ block-item* }`
//!
//! Control-flow statements (`if`, `while`, `for`, `switch`, `break`,
//! `continue`, `return`, `goto`, labels) live in sibling tasks 05-14
//! through 05-17; each plugs a new match arm into [`parse_stmt`]
//! before the expression-statement fallthrough.
//!
//! ## Block items and scoping
//!
//! C99 §6.8.2 lets declarations appear anywhere inside a compound
//! statement, interleaved with statements — the well-known C89
//! "decls must come first" rule is gone. The AST already models
//! this with [`BlockItem::{Decl, Stmt}`]. Declaration parsing
//! (task 05-18+) is not yet implemented, so [`parse_block_item`]
//! currently routes every item through [`parse_stmt`] and wraps the
//! result in `BlockItem::Stmt`. A source line like `int x;` inside a
//! block therefore hits the expression-statement fallthrough, which
//! will emit a diagnostic at the `int` keyword — this is a known
//! deferral, not a regression.
//!
//! Each compound statement pushes a new scope on the parser's
//! [`ScopeStack`] on entry and pops it on exit so the typedef-name
//! lookup table tracks lexical nesting. The push/pop is matched in
//! every return path, including the error-recovery path where the
//! block ends at end-of-input without a closing `}` — otherwise a
//! malformed input would leave later parts of the translation unit
//! resolving names against an inner scope and wreck the typedef
//! hack.
//!
//! ## Error recovery
//!
//! - A missing opening `{` in [`parse_block`] is a hard failure: the
//!   caller asked for a block, we don't have one, nothing useful to
//!   return.
//! - A missing closing `}` is reported with the opening brace as
//!   the labelled primary span and the block is closed at the end
//!   of input so the enclosing statement/translation-unit parse can
//!   keep making progress.
//! - A missing terminating `;` after an expression statement is
//!   reported but the statement is still returned — the parser is
//!   otherwise in a good state and discarding the expression would
//!   amplify the error.
//! - A block item that fails to parse (inner `parse_stmt` returned
//!   `None`) advances the cursor by at least one token before the
//!   next iteration so the block loop cannot spin forever on the
//!   offending token.

use rcc_ast::{Block, BlockItem, Stmt, StmtKind};
use rcc_lexer::Punct;

use crate::expr::parse_expression;
use crate::token::TokenKind;
use crate::Parser;

/// Parse a C99 §6.8 *statement*.
///
/// Dispatches on the current token:
///
/// - `{` → [`parse_block`] and wrap it as [`StmtKind::Compound`].
/// - `;` → consume and return [`StmtKind::Null`].
/// - anything else → [`parse_expression`] followed by a required
///   `;`, wrapped as [`StmtKind::Expr(Some(..))`].
///
/// Control-flow statements (`if`, `while`, etc.) will insert their
/// own match arms above the expression-statement fallthrough in
/// tasks 05-14..05-17.
///
/// Returns `None` at end-of-input or when the expression-statement
/// path fails to produce an expression (in which case a diagnostic
/// has already been emitted by [`parse_expression`] and the cursor
/// is left on the offending token).
pub fn parse_stmt(p: &mut Parser<'_>) -> Option<Stmt> {
    let tok = p.peek()?;
    match &tok.kind {
        TokenKind::Punct(Punct::LBrace) => {
            let block = parse_block(p)?;
            let span = block.span;
            let id = p.fresh_id();
            Some(Stmt { id, kind: StmtKind::Compound(block), span })
        }
        TokenKind::Punct(Punct::Semi) => {
            let span = tok.span;
            p.bump();
            let id = p.fresh_id();
            Some(Stmt { id, kind: StmtKind::Null, span })
        }
        _ => parse_expr_stmt(p),
    }
}

/// Parse an expression statement: `expression ;`.
///
/// A missing `;` is diagnosed but the already-parsed expression is
/// still wrapped and returned — see the module-level "Error
/// recovery" notes.
fn parse_expr_stmt(p: &mut Parser<'_>) -> Option<Stmt> {
    let expr = parse_expression(p)?;
    let expr_span = expr.span;
    let end_span = match p.peek() {
        Some(t) if matches!(t.kind, TokenKind::Punct(Punct::Semi)) => {
            let s = t.span;
            p.bump();
            s
        }
        _ => {
            let at = p.cur_span();
            p.session.handler.struct_err(at, "expected `;` after expression").emit();
            expr_span
        }
    };
    let id = p.fresh_id();
    Some(Stmt { id, kind: StmtKind::Expr(Some(expr)), span: expr_span.to(end_span) })
}

/// Parse a C99 §6.8.2 *compound-statement*: `{ block-item* }`.
///
/// Pushes a new scope on entry and pops it on exit, on every return
/// path (including error recovery). Callers are responsible for the
/// surrounding statement wrapping — [`parse_stmt`] does the
/// `StmtKind::Compound(..)` wrap, while function-definition parsing
/// (task 05-25) consumes the `Block` directly for `FunctionDef::body`.
///
/// Returns `None` if the current token isn't `{`.
pub fn parse_block(p: &mut Parser<'_>) -> Option<Block> {
    let tok = p.peek()?;
    let lbrace_span = match tok.kind {
        TokenKind::Punct(Punct::LBrace) => {
            let s = tok.span;
            p.bump();
            s
        }
        _ => {
            let at = tok.span;
            p.session.handler.struct_err(at, "expected `{` to start block").emit();
            return None;
        }
    };

    p.scopes.push();

    let mut items = Vec::new();
    let rbrace_span = loop {
        match p.peek() {
            None => {
                p.session
                    .handler
                    .struct_err(p.cur_span(), "unexpected end of input inside block")
                    .label(lbrace_span, "unclosed `{` here")
                    .emit();
                break lbrace_span;
            }
            Some(t) if matches!(t.kind, TokenKind::Punct(Punct::RBrace)) => {
                let s = t.span;
                p.bump();
                break s;
            }
            _ => {
                let before = p.cursor;
                if let Some(item) = parse_block_item(p) {
                    items.push(item);
                } else if p.cursor == before {
                    // parse_block_item emitted a diagnostic but did
                    // not consume anything; advance one token to
                    // guarantee loop progress.
                    p.bump();
                }
            }
        }
    };

    p.scopes.pop();

    let id = p.fresh_id();
    Some(Block { id, items, span: lbrace_span.to(rbrace_span) })
}

/// Parse one *block-item* (C99 §6.8.2).
///
/// Grammar:
///
/// ```text
/// block-item:
///     declaration
///     statement
/// ```
///
/// Declaration parsing lands in tasks 05-18+; for now every item is
/// routed through [`parse_stmt`] and wrapped as `BlockItem::Stmt`.
/// A line like `int x;` therefore reaches the expression-statement
/// fallthrough and is diagnosed as "expected primary expression" —
/// a deliberate stand-in until the declaration parser lands.
pub fn parse_block_item(p: &mut Parser<'_>) -> Option<BlockItem> {
    let stmt = parse_stmt(p)?;
    Some(BlockItem::Stmt(Box::new(stmt)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phase7::convert;
    use rcc_ast::ExprKind;
    use rcc_lexer::{PpToken, PpTokenKind, Punct as LexPunct};
    use rcc_session::Session;
    use rcc_span::{BytePos, FileId, Span};
    use std::sync::Arc;

    fn mk_session(src: &str) -> (Session, FileId, rcc_errors::CaptureEmitter) {
        let (sess, cap) = Session::for_test();
        let fid =
            sess.source_map.write().unwrap().add_file("t.c".into(), Arc::from(src.to_owned()));
        (sess, fid, cap)
    }

    fn pp(kind: PpTokenKind, file: FileId, lo: u32, hi: u32) -> PpToken {
        PpToken {
            kind,
            span: Span::new(file, BytePos(lo), BytePos(hi)),
            leading_ws: false,
            at_line_start: false,
        }
    }

    // ── parse_stmt: null + expression statements ─────────────────────

    #[test]
    fn null_statement_parses() {
        // Bare `;` is a null statement (§6.8.3 ¶1).
        let src = ";";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = [pp(PpTokenKind::Punct(LexPunct::Semi), fid, 0, 1)];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let s = parse_stmt(&mut parser).expect("`;` parses");
        assert!(matches!(s.kind, StmtKind::Null));
        assert_eq!(s.span.lo.0, 0);
        assert_eq!(s.span.hi.0, 1);
        assert_eq!(parser.cursor, 1);
        assert!(cap.diagnostics().is_empty(), "no diagnostics for `;`");
    }

    #[test]
    fn expression_statement_wraps_expression_and_eats_semi() {
        // `a;` → Expr(Some(Ident(a))) with span covering `a;`.
        let src = "a;";
        let (mut sess, fid, cap) = mk_session(src);
        let pps =
            [pp(PpTokenKind::Ident, fid, 0, 1), pp(PpTokenKind::Punct(LexPunct::Semi), fid, 1, 2)];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let s = parse_stmt(&mut parser).expect("`a;` parses");
        match s.kind {
            StmtKind::Expr(Some(e)) => match e.kind {
                ExprKind::Ident(sym) => {
                    assert_eq!(parser.session.interner.get(sym), "a");
                }
                other => panic!("expected Ident, got {other:?}"),
            },
            other => panic!("expected Expr(Some(..)), got {other:?}"),
        }
        assert_eq!(s.span.lo.0, 0);
        assert_eq!(s.span.hi.0, 2);
        assert_eq!(parser.cursor, 2);
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn expression_statement_without_semi_is_diagnosed_but_returned() {
        // `a` (no `;`): the statement is still returned so the
        // caller makes progress; a diagnostic reports the missing
        // terminator.
        let src = "a";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = [pp(PpTokenKind::Ident, fid, 0, 1)];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let s = parse_stmt(&mut parser).expect("bare expression still yields a Stmt");
        assert!(matches!(s.kind, StmtKind::Expr(Some(_))));
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "exactly one diagnostic for missing `;`");
        assert!(
            diags[0].message.contains("expected `;`"),
            "diagnostic mentions `;`: {:?}",
            diags[0].message
        );
    }

    // ── parse_block: empty + non-empty + nested ──────────────────────

    #[test]
    fn empty_compound_statement_parses() {
        // `{}` → Compound(Block { items: [] }) with span 0..2.
        let src = "{}";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = [
            pp(PpTokenKind::Punct(LexPunct::LBrace), fid, 0, 1),
            pp(PpTokenKind::Punct(LexPunct::RBrace), fid, 1, 2),
        ];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let start_depth = parser.scopes.depth();
        let s = parse_stmt(&mut parser).expect("`{}` parses");
        match s.kind {
            StmtKind::Compound(block) => {
                assert!(block.items.is_empty(), "empty block has no items");
                assert_eq!(block.span.lo.0, 0);
                assert_eq!(block.span.hi.0, 2);
            }
            other => panic!("expected Compound, got {other:?}"),
        }
        assert_eq!(parser.cursor, 2);
        assert_eq!(parser.scopes.depth(), start_depth, "scope push/pop is matched");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn compound_with_single_expression_statement() {
        // `{ a; }` → Compound with one Expr(Ident(a)) item.
        let src = "{a;}";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = [
            pp(PpTokenKind::Punct(LexPunct::LBrace), fid, 0, 1),
            pp(PpTokenKind::Ident, fid, 1, 2),
            pp(PpTokenKind::Punct(LexPunct::Semi), fid, 2, 3),
            pp(PpTokenKind::Punct(LexPunct::RBrace), fid, 3, 4),
        ];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let start_depth = parser.scopes.depth();
        let s = parse_stmt(&mut parser).expect("`{a;}` parses");
        match s.kind {
            StmtKind::Compound(block) => {
                assert_eq!(block.items.len(), 1);
                match &block.items[0] {
                    BlockItem::Stmt(inner) => match &inner.kind {
                        StmtKind::Expr(Some(e)) => match &e.kind {
                            ExprKind::Ident(sym) => {
                                assert_eq!(parser.session.interner.get(*sym), "a");
                            }
                            other => panic!("expected Ident, got {other:?}"),
                        },
                        other => panic!("expected Expr(Some(..)), got {other:?}"),
                    },
                    BlockItem::Decl(_) => panic!("decl parsing not expected here yet"),
                }
                assert_eq!(block.span.lo.0, 0);
                assert_eq!(block.span.hi.0, 4);
            }
            other => panic!("expected Compound, got {other:?}"),
        }
        assert_eq!(parser.cursor, 4);
        assert_eq!(parser.scopes.depth(), start_depth);
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn nested_compound_statements_preserve_scope_depth() {
        // Acceptance bullet: `{{{}}}` pushes and pops exactly three
        // times; ScopeStack depth before and after must match.
        let src = "{{{}}}";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = [
            pp(PpTokenKind::Punct(LexPunct::LBrace), fid, 0, 1),
            pp(PpTokenKind::Punct(LexPunct::LBrace), fid, 1, 2),
            pp(PpTokenKind::Punct(LexPunct::LBrace), fid, 2, 3),
            pp(PpTokenKind::Punct(LexPunct::RBrace), fid, 3, 4),
            pp(PpTokenKind::Punct(LexPunct::RBrace), fid, 4, 5),
            pp(PpTokenKind::Punct(LexPunct::RBrace), fid, 5, 6),
        ];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let start_depth = parser.scopes.depth();
        let s = parse_stmt(&mut parser).expect("`{{{}}}` parses");
        // Walk three Compound layers and land on an empty Block.
        let mut cur = s;
        for level in 0..3 {
            cur = match cur.kind {
                StmtKind::Compound(block) => {
                    if level == 2 {
                        assert!(block.items.is_empty(), "innermost block is empty");
                        break;
                    }
                    assert_eq!(block.items.len(), 1, "outer levels wrap exactly one stmt");
                    match block.items.into_iter().next().unwrap() {
                        BlockItem::Stmt(inner) => *inner,
                        BlockItem::Decl(_) => panic!("no decls expected"),
                    }
                }
                other => panic!("expected Compound at level {level}, got {other:?}"),
            };
        }
        assert_eq!(parser.cursor, 6);
        assert_eq!(parser.scopes.depth(), start_depth, "scope depth preserved after nested blocks");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn unclosed_brace_recovers_and_pops_scope() {
        // `{` with no matching `}` — parser must still pop the
        // scope it pushed, emit a diagnostic, and hand back a Block
        // so higher-level parsing can continue.
        let src = "{";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = [pp(PpTokenKind::Punct(LexPunct::LBrace), fid, 0, 1)];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let start_depth = parser.scopes.depth();
        let s = parse_stmt(&mut parser).expect("unclosed `{` still yields a Stmt");
        assert!(matches!(s.kind, StmtKind::Compound(_)));
        assert_eq!(parser.scopes.depth(), start_depth, "scope popped on recovery");
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("end of input"), "diag mentions EOI: {:?}", diags[0]);
    }
}
