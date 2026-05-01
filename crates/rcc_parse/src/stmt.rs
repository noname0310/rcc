//! Statement parsing (C99 §6.8).
//!
//! The dispatcher in [`parse_stmt`] walks the §6.8 statement grammar
//! family by family:
//!
//! - *labeled statement*       (§6.8.1)   — `ident : stmt`, `case …:`,
//!   `default :`
//! - *compound statement*      (§6.8.2)   — `{ block-item* }`
//! - *expression statement*    (§6.8.3)   — `expr ;`  /  `;`
//! - *selection statement*     (§6.8.4)   — `if`, `switch`
//! - *iteration statement*     (§6.8.5)   — `while`, `do`, `for`
//! - *jump statement*          (§6.8.6)   — `goto`, `continue`, `break`,
//!   `return`
//!
//! Tasks 05-13 .. 05-17 landed these in the order above; the dispatch
//! table is a single `match` on the lookahead token that routes to the
//! per-family parser. The expression statement path is the fallthrough.
//!
//! ## Dangling `else` (§6.8.4.1)
//!
//! The grammar is intentionally ambiguous: an `else` after
//! `if ( … ) stmt` could bind either to that `if` or to an outer one.
//! The §6.8.4.1 footnote resolves the ambiguity in favour of the
//! nearest preceding `if`, and we implement that by *greedy* `else`
//! consumption inside [`parse_if_stmt`]. Because a nested `if` in the
//! then-branch is itself produced by a recursive [`parse_stmt`] call,
//! that inner call swallows the `else` before control returns to the
//! outer `if`, which then sees no `else` and yields `else_branch =
//! None`. No additional bookkeeping is required.
//!
//! ## Labels vs expression statements (§6.8.1)
//!
//! A labeled statement of the shape `ident : stmt` is disambiguated
//! from a bare identifier expression by two-token lookahead: if the
//! token after the leading `Ident` is `:`, the item is a label; any
//! other trailer (including `;`, `=`, binary operators, postfix
//! trailers) falls through to the expression-statement path. C99 has
//! no expression production starting with `ident :`, so the
//! lookahead is unambiguous.
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
//! The same stub applies to the `for (…;…;…)` *init* clause: C99
//! §6.8.5p3 permits either an expression statement or a declaration
//! there, but only the expression form is accepted by
//! [`parse_for_stmt`] until the declaration parser arrives. A `for`
//! whose init is `int i = 0` still enters a fresh scope (we push one
//! unconditionally on `(`), but the `int` keyword reaches the
//! expression-statement path and is diagnosed as such. Once task
//! 05-18 lands, the init slot will route through [`parse_block_item`]
//! and both shapes will be accepted uniformly.
//!
//! Each compound statement pushes a new scope on the parser's
//! [`ScopeStack`] on entry and pops it on exit so the typedef-name
//! lookup table tracks lexical nesting. The push/pop is matched in
//! every return path, including the error-recovery path where the
//! block ends at end-of-input without a closing `}` — otherwise a
//! malformed input would leave later parts of the translation unit
//! resolving names against an inner scope and wreck the typedef
//! hack. `for (…;…;…) body` uses the same discipline: a scope is
//! pushed before the init and popped after the body so any
//! declarations in the init clause (once supported) are visible only
//! inside the loop.
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
//! - A missing terminating `;` after an expression / `break` /
//!   `continue` / `return` / `goto` is reported but the statement is
//!   still returned — the parser is otherwise in a good state and
//!   discarding the statement would amplify the error.
//! - A missing `(` / `)` around a control-flow condition is a hard
//!   failure for that specific statement (returns `None`) because
//!   without the parentheses we cannot safely locate the body.
//! - A block item that fails to parse (inner `parse_stmt` returned
//!   `None`) advances the cursor by at least one token before the
//!   next iteration so the block loop cannot spin forever on the
//!   offending token.

use rcc_ast::{Block, BlockItem, Stmt, StmtKind};
use rcc_lexer::Punct;
use rcc_span::{Span, Symbol};

use crate::expr::{parse_assignment_expression, parse_expression};
use crate::keywords::Keyword;
use crate::token::TokenKind;
use crate::Parser;

/// Parse a C99 §6.8 *statement*.
///
/// Dispatches on the lookahead token (see the family list in the
/// module docstring). The expression-statement path is the default
/// fallthrough; a lone `;` is recognised as the null statement.
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
        TokenKind::Keyword(Keyword::If) => parse_if_stmt(p),
        TokenKind::Keyword(Keyword::While) => parse_while_stmt(p),
        TokenKind::Keyword(Keyword::Do) => parse_do_while_stmt(p),
        TokenKind::Keyword(Keyword::For) => parse_for_stmt(p),
        TokenKind::Keyword(Keyword::Switch) => parse_switch_stmt(p),
        TokenKind::Keyword(Keyword::Case) => parse_case_stmt(p),
        TokenKind::Keyword(Keyword::Default) => parse_default_stmt(p),
        TokenKind::Keyword(Keyword::Break) => parse_break_stmt(p),
        TokenKind::Keyword(Keyword::Continue) => parse_continue_stmt(p),
        TokenKind::Keyword(Keyword::Return) => parse_return_stmt(p),
        TokenKind::Keyword(Keyword::Goto) => parse_goto_stmt(p),
        TokenKind::Ident(_) if peek_is_label(p) => parse_labeled_stmt(p),
        _ => parse_expr_stmt(p),
    }
}

/// Return `true` if the cursor is on an `Ident` followed immediately
/// by `:` — the shape of a labeled statement (C99 §6.8.1). The caller
/// has already matched the `Ident` arm; this helper only checks the
/// trailer. Any other token after the `Ident` (including none) leaves
/// the label arm and falls through to the expression-statement path.
fn peek_is_label(p: &Parser<'_>) -> bool {
    matches!(p.tokens.get(p.cursor + 1).map(|t| &t.kind), Some(TokenKind::Punct(Punct::Colon)))
}

/// Parse an expression statement: `expression ;`.
///
/// A missing `;` is diagnosed but the already-parsed expression is
/// still wrapped and returned — see the module-level "Error
/// recovery" notes.
fn parse_expr_stmt(p: &mut Parser<'_>) -> Option<Stmt> {
    let expr = parse_expression(p)?;
    let expr_span = expr.span;
    let end_span = expect_semi(p, expr_span, "expected `;` after expression");
    let id = p.fresh_id();
    Some(Stmt { id, kind: StmtKind::Expr(Some(expr)), span: expr_span.to(end_span) })
}

/// Consume a terminating `;` and return its span. If the current
/// token isn't a `;`, emit `msg` at the current cursor position and
/// return `fallback_end` so the caller can still synthesise a span
/// for the statement. This mirrors the §6.8.3 recovery shape used by
/// the expression-statement path and is reused by every jump
/// statement that requires a trailing `;`.
fn expect_semi(p: &mut Parser<'_>, fallback_end: Span, msg: &str) -> Span {
    match p.peek() {
        Some(t) if matches!(t.kind, TokenKind::Punct(Punct::Semi)) => {
            let s = t.span;
            p.bump();
            s
        }
        _ => {
            let at = p.cur_span();
            p.session.handler.struct_err(at, msg).emit();
            fallback_end
        }
    }
}

/// Consume a specific punctuator at the cursor. On success return its
/// span; on failure emit a diagnostic pointing at the cursor and
/// return `None`. Used by the `(` / `)` pairs around control-flow
/// conditions, where a failure to find the expected bracket is bad
/// enough that we cannot reliably locate the body and so have to
/// abort the whole statement parse.
fn expect_punct(p: &mut Parser<'_>, want: Punct, msg: &str) -> Option<Span> {
    match p.peek() {
        Some(t) if matches!(t.kind, TokenKind::Punct(pp) if pp == want) => {
            let s = t.span;
            p.bump();
            Some(s)
        }
        _ => {
            let at = p.cur_span();
            p.session.handler.struct_err(at, msg).emit();
            None
        }
    }
}

/// Parse a C99 §6.8.4.1 *selection-statement* opening with `if`.
///
/// Grammar: `if ( expression ) statement [ else statement ]`.
///
/// Dangling-else is resolved greedily — the recursive call that
/// produces the then-branch will itself consume any `else` that
/// immediately follows it, so an outer `if` only observes an `else`
/// when no inner `if` could claim it. This implements the §6.8.4.1
/// footnote binding rule without any explicit state.
fn parse_if_stmt(p: &mut Parser<'_>) -> Option<Stmt> {
    let if_span = p.cur_span();
    p.bump(); // `if`
    expect_punct(p, Punct::LParen, "expected `(` after `if`")?;
    let cond = parse_expression(p)?;
    expect_punct(p, Punct::RParen, "expected `)` after `if` condition")?;
    let then_branch = Box::new(parse_stmt(p)?);

    let (else_branch, end_span) =
        if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Keyword(Keyword::Else))) {
            p.bump(); // `else`
            let eb = parse_stmt(p)?;
            let span = eb.span;
            (Some(Box::new(eb)), span)
        } else {
            (None, then_branch.span)
        };

    let id = p.fresh_id();
    Some(Stmt {
        id,
        kind: StmtKind::If { cond, then_branch, else_branch },
        span: if_span.to(end_span),
    })
}

/// Parse a C99 §6.8.5 `while` iteration statement:
/// `while ( expression ) statement`.
fn parse_while_stmt(p: &mut Parser<'_>) -> Option<Stmt> {
    let while_span = p.cur_span();
    p.bump(); // `while`
    expect_punct(p, Punct::LParen, "expected `(` after `while`")?;
    let cond = parse_expression(p)?;
    expect_punct(p, Punct::RParen, "expected `)` after `while` condition")?;
    let body = Box::new(parse_stmt(p)?);
    let end_span = body.span;
    let id = p.fresh_id();
    Some(Stmt { id, kind: StmtKind::While { cond, body }, span: while_span.to(end_span) })
}

/// Parse a C99 §6.8.5 `do` iteration statement:
/// `do statement while ( expression ) ;`.
///
/// A missing `while` after the body, or a missing terminating `;`,
/// is reported but does not prevent a best-effort AST node from
/// being produced so the surrounding block can continue parsing.
fn parse_do_while_stmt(p: &mut Parser<'_>) -> Option<Stmt> {
    let do_span = p.cur_span();
    p.bump(); // `do`
    let body = Box::new(parse_stmt(p)?);

    match p.peek() {
        Some(t) if matches!(t.kind, TokenKind::Keyword(Keyword::While)) => {
            p.bump();
        }
        _ => {
            let at = p.cur_span();
            p.session.handler.struct_err(at, "expected `while` after `do` body").emit();
            return None;
        }
    }
    expect_punct(p, Punct::LParen, "expected `(` after `while`")?;
    let cond = parse_expression(p)?;
    let rparen = expect_punct(p, Punct::RParen, "expected `)` after `while` condition")?;
    let end_span = expect_semi(p, rparen, "expected `;` after `do-while`");
    let id = p.fresh_id();
    Some(Stmt { id, kind: StmtKind::DoWhile { body, cond }, span: do_span.to(end_span) })
}

/// Parse a C99 §6.8.5 `for` iteration statement:
/// `for ( init? ; cond? ; step? ) statement`.
///
/// **Init clause:** C99 §6.8.5p3 allows either an expression
/// statement or a declaration. A `for` with an empty init (`for ( ;
/// ...)`) is supported; a declaration init is parsed as
/// `BlockItem::Decl` so HIR lowering can give it the loop scope.
///
/// A new scope is pushed on entry (covering the init, cond, step
/// and body) and popped on every return path so the future
/// declaration form will naturally scope its names to the loop.
fn parse_for_stmt(p: &mut Parser<'_>) -> Option<Stmt> {
    let for_span = p.cur_span();
    p.bump(); // `for`
              // A missing `(` is a hard failure *before* we push the scope —
              // we have nothing to unwind and the `?` propagation is clean.
    expect_punct(p, Punct::LParen, "expected `(` after `for`")?;

    p.scopes.push();

    // ── init ────────────────────────────────────────────────────────
    let init: Option<Box<BlockItem>> =
        if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Semi))) {
            p.bump();
            None
        } else if looks_like_decl(p) {
            let decl = match crate::decl::parse_declaration(p) {
                Some(decl) => decl,
                None => {
                    p.scopes.pop();
                    return None;
                }
            };
            Some(Box::new(BlockItem::Decl(decl)))
        } else {
            // parse_expr_stmt already handles the trailing `;` so we
            // wrap its result as a BlockItem::Stmt.
            let stmt = match parse_expr_stmt(p) {
                Some(s) => s,
                None => {
                    p.scopes.pop();
                    return None;
                }
            };
            Some(Box::new(BlockItem::Stmt(Box::new(stmt))))
        };

    // ── cond ────────────────────────────────────────────────────────
    //
    // Unlike the init slot, the cond slot is *not* an expression
    // statement — the grammar spells the separator as a bare `;`
    // after an optional expression — so we parse the expression (if
    // any) ourselves and then require the trailing `;`. An empty
    // cond is represented as `None`; HIR lowering treats that as
    // "always true" per §6.8.5.3 ¶2.
    let cond_fallback = p.cur_span();
    let cond = if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Semi))) {
        None
    } else {
        let e = match parse_expression(p) {
            Some(e) => e,
            None => {
                p.scopes.pop();
                return None;
            }
        };
        Some(e)
    };
    let _ = expect_semi(p, cond_fallback, "expected `;` after `for` condition");

    // ── step ────────────────────────────────────────────────────────
    let step: Option<_> =
        if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::RParen))) {
            None
        } else {
            let e = match parse_expression(p) {
                Some(e) => e,
                None => {
                    p.scopes.pop();
                    return None;
                }
            };
            Some(e)
        };

    // Past this point the scope is pushed, so every early return
    // must pop it — we cannot use `?` here without leaking a scope.
    if expect_punct(p, Punct::RParen, "expected `)` to close `for` header").is_none() {
        p.scopes.pop();
        return None;
    }

    let body = match parse_stmt(p) {
        Some(s) => Box::new(s),
        None => {
            p.scopes.pop();
            return None;
        }
    };
    let end_span = body.span;

    p.scopes.pop();

    let id = p.fresh_id();
    Some(Stmt { id, kind: StmtKind::For { init, cond, step, body }, span: for_span.to(end_span) })
}

/// Parse a C99 §6.8.4.2 `switch` statement:
/// `switch ( expression ) statement`.
///
/// `case` / `default` attachment to the correct switch body is
/// entirely positional: each `case`/`default` label is a statement
/// in its own right (see [`parse_case_stmt`] /
/// [`parse_default_stmt`]) and wraps its inner statement, so a
/// nested `switch` inside the outer switch body starts a new
/// "case-scope" simply by being the innermost `switch` the inner
/// `case` labels reach during top-down parsing. No side-table is
/// needed at the parser level — the AST shape itself encodes the
/// nesting, and HIR lowering walks it accordingly.
fn parse_switch_stmt(p: &mut Parser<'_>) -> Option<Stmt> {
    let sw_span = p.cur_span();
    p.bump(); // `switch`
    expect_punct(p, Punct::LParen, "expected `(` after `switch`")?;
    let cond = parse_expression(p)?;
    expect_punct(p, Punct::RParen, "expected `)` after `switch` condition")?;
    let body = Box::new(parse_stmt(p)?);
    let end_span = body.span;
    let id = p.fresh_id();
    Some(Stmt { id, kind: StmtKind::Switch { cond, body }, span: sw_span.to(end_span) })
}

/// Parse a C99 §6.8.1 labeled statement of the `case` kind:
/// `case constant-expression : statement`.
///
/// The constant-expression is an *assignment-expression* (§6.5.16)
/// with `,` intentionally excluded — C99 §6.6 defines a
/// constant-expression as a conditional-expression, and a
/// conditional-expression is a strict subset of
/// assignment-expression. Constness evaluation itself lives in
/// typeck; the parser only checks shape.
fn parse_case_stmt(p: &mut Parser<'_>) -> Option<Stmt> {
    let case_span = p.cur_span();
    p.bump(); // `case`
    let value = parse_assignment_expression(p)?;
    expect_punct(p, Punct::Colon, "expected `:` after `case` value")?;
    let body = Box::new(parse_stmt(p)?);
    let end_span = body.span;
    let id = p.fresh_id();
    Some(Stmt { id, kind: StmtKind::Case { value, body }, span: case_span.to(end_span) })
}

/// Parse a C99 §6.8.1 labeled statement of the `default` kind:
/// `default : statement`. Ordering relative to `case` labels is
/// unconstrained at the parse level; the HIR-level switch check
/// rejects multiple `default`s per switch once that phase lands.
fn parse_default_stmt(p: &mut Parser<'_>) -> Option<Stmt> {
    let def_span = p.cur_span();
    p.bump(); // `default`
    expect_punct(p, Punct::Colon, "expected `:` after `default`")?;
    let body = Box::new(parse_stmt(p)?);
    let end_span = body.span;
    let id = p.fresh_id();
    Some(Stmt { id, kind: StmtKind::Default { body }, span: def_span.to(end_span) })
}

/// Parse a C99 §6.8.1 labeled statement of the `ident :` kind:
/// `identifier : statement`. Called only when the lookahead in
/// [`parse_stmt`] has already confirmed the `Ident :` shape, so
/// both tokens are guaranteed to be present here.
fn parse_labeled_stmt(p: &mut Parser<'_>) -> Option<Stmt> {
    let label_tok = p.bump().expect("label ident present by lookahead");
    let name = match label_tok.kind {
        TokenKind::Ident(sym) => sym,
        _ => unreachable!("parse_stmt routed non-Ident through label path"),
    };
    let label_span = label_tok.span;
    // `:` — guaranteed present by the lookahead in parse_stmt.
    p.bump();
    let body = Box::new(parse_stmt(p)?);
    let end_span = body.span;
    let id = p.fresh_id();
    Some(Stmt { id, kind: StmtKind::Label { name, body }, span: label_span.to(end_span) })
}

/// Parse a C99 §6.8.6 `break ;` jump statement. Whether the
/// surrounding context is a loop or a switch is **not** checked
/// here; that diagnostic is produced during HIR construction when
/// the break-target stack is known. Without loop/switch context the
/// parser still yields a `Break` node so the surrounding block
/// structure is preserved for later error reporting.
fn parse_break_stmt(p: &mut Parser<'_>) -> Option<Stmt> {
    let kw_span = p.cur_span();
    p.bump(); // `break`
    let end_span = expect_semi(p, kw_span, "expected `;` after `break`");
    let id = p.fresh_id();
    Some(Stmt { id, kind: StmtKind::Break, span: kw_span.to(end_span) })
}

/// Parse a C99 §6.8.6 `continue ;` jump statement. Same caveat as
/// [`parse_break_stmt`] — the "inside a loop" check is deferred to
/// HIR lowering.
fn parse_continue_stmt(p: &mut Parser<'_>) -> Option<Stmt> {
    let kw_span = p.cur_span();
    p.bump(); // `continue`
    let end_span = expect_semi(p, kw_span, "expected `;` after `continue`");
    let id = p.fresh_id();
    Some(Stmt { id, kind: StmtKind::Continue, span: kw_span.to(end_span) })
}

/// Parse a C99 §6.8.6 `return [expr] ;` jump statement.
///
/// An immediately-following `;` yields `Return(None)` per §6.8.6p3;
/// anything else is parsed as a full expression (commas included,
/// since `return a, b;` is legal in C99 — the comma-expression is
/// evaluated and its value is what is returned).
fn parse_return_stmt(p: &mut Parser<'_>) -> Option<Stmt> {
    let kw_span = p.cur_span();
    p.bump(); // `return`
    let (value, end_span) =
        if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Semi))) {
            let s = p.cur_span();
            p.bump();
            (None, s)
        } else {
            let e = parse_expression(p)?;
            let e_span = e.span;
            let s = expect_semi(p, e_span, "expected `;` after `return` value");
            (Some(e), s)
        };
    let id = p.fresh_id();
    Some(Stmt { id, kind: StmtKind::Return(value), span: kw_span.to(end_span) })
}

/// Parse a C99 §6.8.6 `goto identifier ;` jump statement. Resolving
/// the label to a labeled statement is a later phase's job; the
/// parser only checks syntax.
fn parse_goto_stmt(p: &mut Parser<'_>) -> Option<Stmt> {
    let kw_span = p.cur_span();
    p.bump(); // `goto`
    let name: Symbol = match p.peek() {
        Some(t) => match t.kind {
            TokenKind::Ident(sym) => {
                p.bump();
                sym
            }
            _ => {
                let at = t.span;
                p.session.handler.struct_err(at, "expected label identifier after `goto`").emit();
                return None;
            }
        },
        None => {
            let at = p.cur_span();
            p.session.handler.struct_err(at, "expected label identifier after `goto`").emit();
            return None;
        }
    };
    let end_span = expect_semi(p, kw_span, "expected `;` after `goto` target");
    let id = p.fresh_id();
    Some(Stmt { id, kind: StmtKind::Goto(name), span: kw_span.to(end_span) })
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
                let err_before = p.session.handler.error_count();
                if let Some(item) = parse_block_item(p) {
                    items.push(item);
                } else if p.cursor == before {
                    // parse_block_item could not make sense of the
                    // current token. Emit a diagnostic (if none was
                    // already emitted at this position) and skip to
                    // the next `;` or `}` so that subsequent items
                    // still get a chance to parse.
                    if p.session.handler.error_count() == err_before {
                        let at = p.cur_span();
                        p.session
                            .handler
                            .struct_err(at, "unexpected token in block")
                            .code(rcc_errors::codes::E0030)
                            .emit();
                    }
                    p.recover_to_sync();
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
/// Declarations are recognised by attempting [`crate::decl::parse_declaration`]
/// first; if the cursor is not at the start of a declaration, it
/// falls through to the statement path.
pub fn parse_block_item(p: &mut Parser<'_>) -> Option<BlockItem> {
    if looks_like_decl(p) {
        if let Some(decl) = crate::decl::parse_declaration(p) {
            return Some(BlockItem::Decl(decl));
        }
    }
    let stmt = parse_stmt(p)?;
    Some(BlockItem::Stmt(Box::new(stmt)))
}

/// Heuristic: does the current token look like it starts a
/// declaration? Used by [`parse_block_item`] to decide whether to
/// try the declaration path before falling through to statements.
fn looks_like_decl(p: &Parser<'_>) -> bool {
    match p.peek() {
        Some(tok) => match &tok.kind {
            TokenKind::Keyword(kw) => matches!(
                kw,
                Keyword::Typedef
                    | Keyword::Extern
                    | Keyword::Static
                    | Keyword::Auto
                    | Keyword::Register
                    | Keyword::Void
                    | Keyword::Char
                    | Keyword::Short
                    | Keyword::Int
                    | Keyword::Long
                    | Keyword::Float
                    | Keyword::Double
                    | Keyword::Signed
                    | Keyword::Unsigned
                    | Keyword::Bool
                    | Keyword::Complex
                    | Keyword::Imaginary
                    | Keyword::Const
                    | Keyword::Volatile
                    | Keyword::Restrict
                    | Keyword::Inline
                    | Keyword::Struct
                    | Keyword::Union
                    | Keyword::Enum
            ),
            TokenKind::Ident(sym) => p.scopes.is_typedef(*sym),
            _ => false,
        },
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phase7::convert;
    use rcc_ast::ExprKind;
    use rcc_lexer::{PpToken, PpTokenKind, Punct as LexPunct, Tokenizer};
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

    /// Lex `src` end-to-end via the real [`Tokenizer`] and run the
    /// phase-7 conversion so tests for control-flow statements can
    /// express the program as plain C source instead of a
    /// hand-written `PpToken` array. Whitespace and newlines are
    /// dropped by `convert` just like they are for the real
    /// front-end pipeline.
    fn tokens_from_src(sess: &mut Session, fid: FileId, src: &str) -> Vec<crate::token::Token> {
        let pps: Vec<PpToken> = Tokenizer::new(fid, src).collect();
        convert(sess, &pps)
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

    // ── 05-14: if / else ─────────────────────────────────────────────

    #[test]
    fn if_without_else_parses() {
        let src = "if (a) b;";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let s = parse_stmt(&mut parser).expect("`if (a) b;` parses");
        match s.kind {
            StmtKind::If { cond, then_branch, else_branch } => {
                assert!(matches!(cond.kind, ExprKind::Ident(_)));
                assert!(matches!(then_branch.kind, StmtKind::Expr(Some(_))));
                assert!(else_branch.is_none(), "no else clause present");
            }
            other => panic!("expected If, got {other:?}"),
        }
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn dangling_else_binds_to_innermost_if() {
        // C99 §6.8.4.1 footnote: `else` binds to the nearest
        // preceding `if` that has no `else` yet. Parse
        // `if (a) if (b) x; else y;` and verify the outer `if` has
        // NO else, while the inner one does.
        let src = "if (a) if (b) x; else y;";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let s = parse_stmt(&mut parser).expect("dangling-else fixture parses");
        let (outer_cond, outer_then, outer_else) = match s.kind {
            StmtKind::If { cond, then_branch, else_branch } => (cond, then_branch, else_branch),
            other => panic!("expected outer If, got {other:?}"),
        };
        assert!(
            matches!(outer_cond.kind, ExprKind::Ident(sym) if parser.session.interner.get(sym) == "a"),
            "outer if cond is `a`"
        );
        assert!(outer_else.is_none(), "outer if must have NO else (dangling-else rule)");
        // The outer then-branch is the inner `if`, which DOES have
        // an `else y;` clause.
        match outer_then.kind {
            StmtKind::If { cond, then_branch, else_branch } => {
                assert!(
                    matches!(cond.kind, ExprKind::Ident(sym) if parser.session.interner.get(sym) == "b"),
                    "inner if cond is `b`"
                );
                match then_branch.kind {
                    StmtKind::Expr(Some(e)) => {
                        assert!(
                            matches!(e.kind, ExprKind::Ident(sym) if parser.session.interner.get(sym) == "x")
                        );
                    }
                    other => panic!("inner then-branch is `x;`, got {other:?}"),
                }
                let eb = else_branch.expect("inner if MUST have an else (dangling-else rule)");
                match eb.kind {
                    StmtKind::Expr(Some(e)) => {
                        assert!(
                            matches!(e.kind, ExprKind::Ident(sym) if parser.session.interner.get(sym) == "y")
                        );
                    }
                    other => panic!("inner else-branch is `y;`, got {other:?}"),
                }
            }
            other => panic!("outer then-branch must be the inner If, got {other:?}"),
        }
        assert!(cap.diagnostics().is_empty(), "clean parse: {:?}", cap.diagnostics());
    }

    // ── 05-15: while / do-while / for ────────────────────────────────

    #[test]
    fn while_loop_parses() {
        let src = "while (a) b;";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let s = parse_stmt(&mut parser).expect("`while (a) b;` parses");
        assert!(matches!(s.kind, StmtKind::While { .. }));
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn do_while_loop_parses() {
        let src = "do { a; } while (b);";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let s = parse_stmt(&mut parser).expect("`do {…} while(…);` parses");
        match s.kind {
            StmtKind::DoWhile { body, cond } => {
                assert!(matches!(body.kind, StmtKind::Compound(_)));
                assert!(matches!(cond.kind, ExprKind::Ident(_)));
            }
            other => panic!("expected DoWhile, got {other:?}"),
        }
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn for_loop_empty_header_parses_with_break_body() {
        // Acceptance: `for (;;) break;` parses. This also exercises
        // the empty init/cond/step path and verifies `break;` at a
        // statement position inside a loop.
        let src = "for (;;) break;";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let start_depth = parser.scopes.depth();
        let s = parse_stmt(&mut parser).expect("`for (;;) break;` parses");
        match s.kind {
            StmtKind::For { init, cond, step, body } => {
                assert!(init.is_none(), "empty init");
                assert!(cond.is_none(), "empty cond");
                assert!(step.is_none(), "empty step");
                assert!(matches!(body.kind, StmtKind::Break), "body is `break;`");
            }
            other => panic!("expected For, got {other:?}"),
        }
        assert_eq!(parser.scopes.depth(), start_depth, "for-init scope popped");
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn for_loop_with_expression_init_parses() {
        let src = "for (i = 0; i < n; i = i + 1) a;";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let s = parse_stmt(&mut parser).expect("expression-init for-loop parses");
        match s.kind {
            StmtKind::For { init, cond, step, body: _ } => {
                let init = init.expect("init present");
                match *init {
                    BlockItem::Stmt(inner) => {
                        assert!(matches!(inner.kind, StmtKind::Expr(Some(_))));
                    }
                    BlockItem::Decl(_) => panic!("expression-init should not parse as a decl"),
                }
                assert!(cond.is_some());
                assert!(step.is_some());
            }
            other => panic!("expected For, got {other:?}"),
        }
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn for_loop_with_declaration_init_parses() {
        let src = "for (int i = 0; i < n; i = i + 1) ;";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let s = parse_stmt(&mut parser).expect("declaration-init for-loop parses");
        match s.kind {
            StmtKind::For { init, cond, step, body: _ } => {
                let init = init.expect("init present");
                match *init {
                    BlockItem::Decl(decl) => {
                        assert_eq!(decl.inits.len(), 1);
                    }
                    BlockItem::Stmt(_) => panic!("declaration-init should parse as a decl"),
                }
                assert!(cond.is_some());
                assert!(step.is_some());
            }
            other => panic!("expected For, got {other:?}"),
        }
        assert!(cap.diagnostics().is_empty());
    }

    // ── 05-16: switch / case / default ───────────────────────────────

    #[test]
    fn switch_with_case_and_default_parses() {
        let src = "switch (x) { case 1: a; case 2: b; default: c; }";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let s = parse_stmt(&mut parser).expect("switch with cases parses");
        let body = match s.kind {
            StmtKind::Switch { body, .. } => body,
            other => panic!("expected Switch, got {other:?}"),
        };
        let block = match body.kind {
            StmtKind::Compound(b) => b,
            other => panic!("switch body must be a compound, got {other:?}"),
        };
        assert_eq!(block.items.len(), 3, "three labeled statements in the body");
        // First is `case 1: a;`, last is `default: c;`.
        match &block.items[0] {
            BlockItem::Stmt(s) => assert!(matches!(s.kind, StmtKind::Case { .. })),
            _ => panic!("first item is a Case"),
        }
        match &block.items[2] {
            BlockItem::Stmt(s) => assert!(matches!(s.kind, StmtKind::Default { .. })),
            _ => panic!("last item is a Default"),
        }
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn nested_switch_attaches_cases_to_correct_body() {
        // Acceptance for 05-16: in a nested switch, the inner
        // `case` labels live under the inner switch body. AST
        // shape is enough to prove attachment because each
        // Case/Default node is literally a child of whichever
        // statement encloses it.
        let src = "switch (a) { case 1: switch (b) { case 2: x; default: y; } case 3: z; }";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let s = parse_stmt(&mut parser).expect("nested switch parses");
        let outer_body = match s.kind {
            StmtKind::Switch { body, .. } => body,
            other => panic!("expected outer Switch, got {other:?}"),
        };
        let outer_block = match outer_body.kind {
            StmtKind::Compound(b) => b,
            other => panic!("outer switch body must be a compound, got {other:?}"),
        };
        // Outer block has exactly two top-level labeled statements:
        // `case 1:` (wrapping the inner switch) and `case 3:`.
        assert_eq!(outer_block.items.len(), 2, "outer switch sees exactly 2 labeled stmts");
        let case1 = match &outer_block.items[0] {
            BlockItem::Stmt(s) => s.as_ref(),
            _ => panic!("case1 item"),
        };
        let case1_body = match &case1.kind {
            StmtKind::Case { body, .. } => body,
            other => panic!("expected Case, got {other:?}"),
        };
        // That inner case's body is the inner Switch — and the
        // inner switch's compound body has BOTH `case 2:` and
        // `default:` attached to IT, not to the outer switch.
        let inner_switch_body = match &case1_body.kind {
            StmtKind::Switch { body, .. } => body,
            other => panic!("case 1 wraps the inner Switch, got {other:?}"),
        };
        let inner_block = match &inner_switch_body.kind {
            StmtKind::Compound(b) => b,
            other => panic!("inner switch body must be a compound, got {other:?}"),
        };
        assert_eq!(inner_block.items.len(), 2, "inner switch has 2 labeled stmts");
        match &inner_block.items[0] {
            BlockItem::Stmt(st) => assert!(matches!(st.kind, StmtKind::Case { .. })),
            _ => panic!(),
        }
        match &inner_block.items[1] {
            BlockItem::Stmt(st) => assert!(matches!(st.kind, StmtKind::Default { .. })),
            _ => panic!(),
        }
        // And `case 3:` remains attached to the OUTER switch body.
        match &outer_block.items[1] {
            BlockItem::Stmt(st) => assert!(matches!(st.kind, StmtKind::Case { .. })),
            _ => panic!(),
        }
        assert!(cap.diagnostics().is_empty(), "clean parse: {:?}", cap.diagnostics());
    }

    // ── 05-17: jumps + labels ────────────────────────────────────────

    #[test]
    fn return_without_value_parses() {
        let src = "return;";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let s = parse_stmt(&mut parser).expect("`return;` parses");
        match s.kind {
            StmtKind::Return(v) => assert!(v.is_none()),
            other => panic!("expected Return(None), got {other:?}"),
        }
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn return_with_value_parses() {
        let src = "return a + b;";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let s = parse_stmt(&mut parser).expect("`return a+b;` parses");
        match s.kind {
            StmtKind::Return(Some(e)) => {
                assert!(matches!(e.kind, ExprKind::Binary { .. }));
            }
            other => panic!("expected Return(Some), got {other:?}"),
        }
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn continue_parses() {
        let src = "continue;";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let s = parse_stmt(&mut parser).expect("`continue;` parses");
        assert!(matches!(s.kind, StmtKind::Continue));
        assert!(cap.diagnostics().is_empty());
    }

    #[test]
    fn bare_break_outside_loop_parses_without_diagnostic() {
        // Acceptance for 05-17: `break;` outside a loop is still
        // accepted at parse time; the "not in a loop" diagnostic
        // is HIR's job.
        let src = "break;";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let s = parse_stmt(&mut parser).expect("`break;` parses");
        assert!(matches!(s.kind, StmtKind::Break));
        assert!(
            cap.diagnostics().is_empty(),
            "parser must not complain about context; HIR does: {:?}",
            cap.diagnostics()
        );
    }

    #[test]
    fn goto_and_label_roundtrip() {
        // Acceptance for 05-17: `goto end; end: ;` parses with the
        // label statement wrapping the null statement `;`.
        let src = "{ goto end; end: ; }";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let s = parse_stmt(&mut parser).expect("goto + label block parses");
        let block = match s.kind {
            StmtKind::Compound(b) => b,
            other => panic!("expected Compound, got {other:?}"),
        };
        assert_eq!(block.items.len(), 2, "goto + labeled null statement");
        // First item: `goto end;` → StmtKind::Goto(end).
        let goto_name = match &block.items[0] {
            BlockItem::Stmt(s) => match &s.kind {
                StmtKind::Goto(sym) => *sym,
                other => panic!("expected Goto, got {other:?}"),
            },
            _ => panic!(),
        };
        assert_eq!(parser.session.interner.get(goto_name), "end");
        // Second item: `end: ;` → Label { name: end, body: Null }.
        match &block.items[1] {
            BlockItem::Stmt(s) => match &s.kind {
                StmtKind::Label { name, body } => {
                    assert_eq!(parser.session.interner.get(*name), "end");
                    assert!(matches!(body.kind, StmtKind::Null), "label wraps the null stmt");
                }
                other => panic!("expected Label, got {other:?}"),
            },
            _ => panic!(),
        }
        assert!(cap.diagnostics().is_empty());
    }

    // ── 05-27: error recovery ────────────────────────────────────────

    #[test]
    fn bad_stmt_produces_three_diagnostics() {
        // Three intentional syntax errors separated by `;`.
        // Each `)` is an unexpected token that the expression
        // parser rejects; the recovery helper skips to `;` so
        // the next bad line gets its own diagnostic.
        let src = "{ ) ; ] ; ) ; }";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let block = parse_block(&mut parser).expect("block still returns despite errors");
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 3, "exactly 3 diagnostics: {diags:?}");
        // The block parsed to completion (didn't panic, hit the `}`).
        assert!(parser.peek().is_none() || matches!(parser.peek().unwrap().kind, TokenKind::Eof));
        // Verify spans cover the block braces.
        assert_eq!(block.span.lo.0, 0);
    }

    #[test]
    fn recovery_does_not_panic_on_malformed_input() {
        // Totally broken input with no sync points — just junk.
        let src = "{ ) ) ) }";
        let (mut sess, fid, _cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        // Should NOT panic, even though every item is unparseable.
        let _block = parse_block(&mut parser);
    }

    #[test]
    fn valid_code_after_error_still_parses() {
        // One bad item followed by a valid expression statement.
        let src = "{ ) ; 42 ; }";
        let (mut sess, fid, cap) = mk_session(src);
        let tokens = tokens_from_src(&mut sess, fid, src);
        let mut parser = Parser::new(&mut sess, tokens);
        let block = parse_block(&mut parser).expect("block parses");
        // Should have 1 good item (42;).
        let stmts: Vec<_> = block
            .items
            .iter()
            .filter(
                |i| matches!(i, BlockItem::Stmt(s) if matches!(s.kind, StmtKind::Expr(Some(_)))),
            )
            .collect();
        assert_eq!(stmts.len(), 1, "the `42;` statement was recovered and parsed");
        // Exactly 1 diagnostic for the `)`.
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "one diagnostic for the `)`: {diags:?}");
    }
}
