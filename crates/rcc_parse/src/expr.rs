//! Expression parsing (C99 §6.5).
//!
//! Task 05-07 landed `parse_primary` — the leaves of the expression
//! grammar. Task 05-08 (this file) adds [`parse_expr_bp`] — a Pratt /
//! precedence-climbing loop driven by [`infix_bp`] that folds binary
//! and assignment operators per the C99 §6.5 table on top of those
//! leaves. The public entry point is [`parse_expression`].
//!
//! Non-goals (filed under later tasks):
//!
//! - Unary / postfix (`++`, `--`, `*`, `&`, `sizeof`, calls, member
//!   access, indexing) — task 05-09.
//! - Cast / `sizeof(type)` — task 05-10.
//! - Conditional `?:` — task 05-11.
//! - Comma `,` — task 05-12.
//!
//! ## Precedence & associativity (C99 §6.5)
//!
//! The table below lists every operator family this task handles,
//! ordered from tightest-binding at the top to loosest at the bottom.
//! The two-column form is the Matklad "left_bp / right_bp" encoding
//! consumed by [`parse_expr_bp`]:
//!
//! ```text
//! family          example            (l_bp, r_bp)   assoc
//! multiplicative  a*b  a/b  a%b      (21, 22)       left
//! additive        a+b  a-b           (19, 20)       left
//! shift           a<<b a>>b          (17, 18)       left
//! relational      a<b  a<=b a>b a>=b (15, 16)       left
//! equality        a==b a!=b          (13, 14)       left
//! bitwise AND     a&b                (11, 12)       left
//! bitwise XOR     a^b                ( 9, 10)       left
//! bitwise OR      a|b                ( 7,  8)       left
//! logical AND     a&&b               ( 5,  6)       left
//! logical OR      a||b               ( 3,  4)       left
//! assignment      a=b a+=b ...       ( 2,  1)       right
//! ```
//!
//! Left-associative operators get `(n, n+1)`: after we consume one, we
//! recurse with `min_bp = n+1`, so a same-family op on the right (which
//! advertises `l_bp = n`) stops the inner recursion and gets bound to
//! the outer left-hand side instead — the classic "left-to-right fold"
//! shape. Right-associative operators (only `=` and its compound
//! cousins in C99) flip the pair to `(n+1, n)` so the inner recursion
//! *does* keep consuming same-family ops, producing the right-leaning
//! `a = (b = c)` tree §6.5.16 mandates.
//!
//! The right-hand side of an assignment still allows any binary
//! operator above it in the table because assignment is the lowest
//! level: `a = b + c` parses as `a = (b + c)` since `+` (l_bp = 19) is
//! greater than the right_bp (1) we recurse with.
//!
//! ## Deep nesting / stack usage
//!
//! `parse_expr_bp` is recursive — each infix operator on the input
//! costs one Rust stack frame. For the expression grammar this is
//! fine: a run like `1+2+3+...` of N operators nests N frames, which
//! comfortably handles the ≥ 32-level acceptance bullet without
//! approaching the default 8 MiB thread stack. Conversion to an
//! iterative shunting-yard variant is a follow-up if pathological
//! inputs ever appear in fuzzing; the iterative shape would need a
//! heap-allocated operator stack anyway, so the recursive version is
//! the right default for hand-written C.
//!
//! ## §6.5.16 lvalue caveat
//!
//! C99 restricts the left-hand side of an assignment to a *unary-
//! expression* grammatically. A precedence-climbing parser cannot
//! express that constraint cheaply — by the time we notice the `=` we
//! have already reduced the LHS through every higher-precedence level.
//! We therefore accept the syntactic superset and defer the lvalue /
//! "modifiable lvalue" checks to semantic analysis (HIR lowering and
//! typeck). Inputs like `(a + b) = c` parse without a parser
//! diagnostic and will be rejected later with a proper "expression is
//! not assignable" error attached to the LHS span. This matches what
//! clang, gcc, and chibicc do.
//!
//! ---
//!
//! Historical notes retained below — the primary-expression grammar
//! this module exposes per §6.5.1:
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
//! compound literals) belong to task 05-09; ternary / cast / unary
//! wiring belongs to 05-09..05-11.
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
//! `( expression )` is parsed by [`parse_primary`] with its inner
//! production delegating to [`parse_expr_bp`] (the Pratt loop landed
//! in 05-08). That covers every binary and assignment operator; the
//! comma operator (§6.5.17) is still outside the loop and arrives in
//! task 05-12 — until then a top-level `,` inside parentheses won't
//! reduce into a `Comma` node, but every `( assignment-expression )`
//! input — which is what every real expression context below the
//! comma operator accepts — parses exactly as the standard requires.
//! The error recovery is simple: on a missing inner expression the
//! outer `Paren` arm returns `None`; on a missing `)` it still
//! returns the inner expression unwrapped (not wrapped in `Paren`)
//! and diagnoses the unbalanced paren so the rest of the token
//! stream is not desynchronised.
//!
//! ## TODO
//!
//! - [ ] 05-12: extend [`parse_expression`] to fold the comma
//!   operator above the assignment level — that task subsumes the
//!   current "assignment-expression" top.
//! - [ ] post-M1: migrate `ExprKind::{Int,Float,Char,String}Lit` to
//!   carry decoded payloads (`IntLiteral`, `FloatLiteral`,
//!   `CharLiteral`, `StringLiteral`) instead of `text: Symbol`.

use rcc_ast::{AssignOp, BinOp, Expr, ExprKind, UnOp};
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
            // The inner production is `assignment-expression` — the
            // top of this task's Pratt loop. Task 05-12 will raise
            // this to the full comma-including expression.
            let inner = parse_expr_bp(p, 0)?;
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

/// Parse a C99 §6.5.3 *unary-expression*.
///
/// Consumes a run of prefix unary operators — `++`, `--`, `+`, `-`,
/// `~`, `!`, `*`, `&` — each of which binds to the *unary-
/// expression* on its right, and then falls through to
/// [`parse_postfix`] which handles the primary plus its postfix
/// trailers. The recursive shape is what makes chains like `&*&x`
/// nest three deep: each operator grabs exactly one more unary on
/// its right before returning.
///
/// Prefix operators are tighter than every binary / assignment
/// operator in [`parse_expr_bp`] and looser than every postfix
/// operator in [`parse_postfix`]. That ordering is enforced
/// *structurally* here, not with binding-power numbers: a prefix
/// wraps whatever unary-expression comes back from the recursive
/// call, which includes the whole postfix loop already fully
/// folded. So `++a--` parses as `++(a--)` because the recursive
/// `parse_prefix_unary` returns `a--` (postfix applied first), then
/// we wrap it in `PreInc`. Both `(++a)--` and `++(a--)` are legal
/// C99 syntax; only the latter is semantically well-typed, but
/// lvalue-ness is checked during HIR lowering / typeck, not here.
///
/// C99 `sizeof` (§6.5.3.4) is also a unary-expression but has two
/// shapes — `sizeof unary-expression` and `sizeof ( type-name )` —
/// whose disambiguation needs typedef-name lookahead; it lands in
/// task 05-10 alongside the cast-expression. This function
/// intentionally does **not** match `Keyword::Sizeof` so that task
/// 05-10 has a clean place to weave in the ambiguity handling.
///
/// Returns `None` only when the inner `parse_postfix` call has
/// already emitted a diagnostic and nothing can be built; a partial
/// prefix chain with a missing operand is still reported rather
/// than silently dropped.
pub fn parse_prefix_unary(p: &mut Parser<'_>) -> Option<Expr> {
    // Peek first so that non-prefix tokens drop through to the
    // postfix / primary path without paying for a speculative bump.
    let tok = p.peek()?;
    let op_span = tok.span;
    let un_op = match tok.kind {
        TokenKind::Punct(Punct::PlusPlus) => UnOp::PreInc,
        TokenKind::Punct(Punct::MinusMinus) => UnOp::PreDec,
        TokenKind::Punct(Punct::Plus) => UnOp::Plus,
        TokenKind::Punct(Punct::Minus) => UnOp::Neg,
        TokenKind::Punct(Punct::Tilde) => UnOp::BitNot,
        TokenKind::Punct(Punct::Bang) => UnOp::LogNot,
        TokenKind::Punct(Punct::Star) => UnOp::Deref,
        TokenKind::Punct(Punct::Amp) => UnOp::AddrOf,
        _ => return parse_postfix(p),
    };
    p.bump();
    // Recurse so chains like `&*&x` / `!!x` / `- -x` nest correctly.
    // A missing operand after a prefix operator has already been
    // diagnosed inside `parse_prefix_unary` (via `parse_postfix` →
    // `parse_primary`); propagating `None` preserves that diagnostic
    // without fabricating a dummy operand span.
    let operand = parse_prefix_unary(p)?;
    let span = op_span.to(operand.span);
    let id = p.fresh_id();
    Some(Expr { id, kind: ExprKind::Unary { op: un_op, operand: Box::new(operand) }, span })
}

/// Parse a C99 §6.5.2 *postfix-expression*: a primary-expression
/// followed by zero or more left-to-right postfix trailers.
///
/// The trailer set is:
///
/// - `a[b]`    — [`ExprKind::Index`] (subscript, §6.5.2.1)
/// - `f(args)` — [`ExprKind::Call`]  (function call, §6.5.2.2)
/// - `a.b`     — [`ExprKind::Member`] (direct member access, §6.5.2.3)
/// - `a->b`    — [`ExprKind::Arrow`] (indirect member access, §6.5.2.3)
/// - `a++`     — [`ExprKind::Unary`] with [`UnOp::PostInc`] (§6.5.2.4)
/// - `a--`     — [`ExprKind::Unary`] with [`UnOp::PostDec`] (§6.5.2.4)
///
/// The loop accumulates them in the order they appear, so
/// `a.b->c[0]++` builds
/// `PostInc(Index(Arrow(Member(a, b), c), 0))` — i.e. strict left-
/// to-right associativity, which is what the §6.5.2 grammar encodes
/// by making *postfix-expression* left-recursive. The primary
/// serves as the loop's seed; each iteration takes the partially-
/// built `lhs` and wraps it in one more layer.
///
/// Compound-literal postfix (`(type){ init }`) is intentionally not
/// recognised here — distinguishing `(type){...}` from `(expr)`
/// requires the type-name lookahead that task 05-10 introduces.
///
/// Returns `None` only when [`parse_primary`] cannot produce an
/// initial expression; mid-chain syntax errors (e.g. `a.` with no
/// field, `f(` with no closing paren) are diagnosed and the
/// partially-built expression is returned so that higher layers can
/// keep parsing.
pub fn parse_postfix(p: &mut Parser<'_>) -> Option<Expr> {
    let mut lhs = parse_primary(p)?;
    // Only punctuators start postfix trailers; extract the `Punct`
    // together with its span up front so the loop body can freely
    // call `p.bump()` / emit diagnostics without fighting the
    // borrow checker over the peeked token.
    while let Some((punct, op_span)) = p.peek().and_then(|t| match t.kind {
        TokenKind::Punct(pu) => Some((pu, t.span)),
        _ => None,
    }) {
        match punct {
            // `a[b]` — §6.5.2.1 array subscript. The bracketed
            // expression is a full expression (comma operator
            // included once task 05-12 lands); for now `parse_expr_bp`
            // at min_bp 0 covers every operator we have.
            Punct::LBracket => {
                let lbracket_span = op_span;
                p.bump();
                let Some(index) = parse_expr_bp(p, 0) else {
                    // Missing / invalid index already diagnosed by
                    // the recursive call. Bail out of the loop so we
                    // don't spin on the same token.
                    break;
                };
                // Close the bracket or diagnose. On a missing `]`
                // we still build the `Index` node so downstream
                // tooling sees the user's intent; the diagnostic
                // labels both the `[` and the current cursor.
                let rbracket_span = match p.peek() {
                    Some(t) if matches!(t.kind, TokenKind::Punct(Punct::RBracket)) => {
                        let s = t.span;
                        p.bump();
                        s
                    }
                    _ => {
                        let at = p.cur_span();
                        p.session
                            .handler
                            .struct_err(at, "expected `]` to close subscript")
                            .label(lbracket_span, "unmatched `[` here")
                            .emit();
                        index.span
                    }
                };
                let span = lhs.span.to(rbracket_span);
                let id = p.fresh_id();
                lhs = Expr {
                    id,
                    kind: ExprKind::Index { base: Box::new(lhs), index: Box::new(index) },
                    span,
                };
            }
            // `a.ident` — direct member access.
            Punct::Dot => {
                p.bump();
                let Some((field, field_span)) = expect_member_ident(p, op_span, ".") else {
                    break;
                };
                let span = lhs.span.to(field_span);
                let id = p.fresh_id();
                lhs = Expr { id, kind: ExprKind::Member { base: Box::new(lhs), field }, span };
            }
            // `a->ident` — indirect member access.
            Punct::Arrow => {
                p.bump();
                let Some((field, field_span)) = expect_member_ident(p, op_span, "->") else {
                    break;
                };
                let span = lhs.span.to(field_span);
                let id = p.fresh_id();
                lhs = Expr { id, kind: ExprKind::Arrow { base: Box::new(lhs), field }, span };
            }
            // `a++` / `a--` — postfix increment / decrement.
            // These MUST be tried before letting `parse_expr_bp`'s
            // infix loop see the token, since `++` is not an infix
            // operator and would otherwise end the expression early.
            Punct::PlusPlus | Punct::MinusMinus => {
                p.bump();
                let un_op = if punct == Punct::PlusPlus { UnOp::PostInc } else { UnOp::PostDec };
                let span = lhs.span.to(op_span);
                let id = p.fresh_id();
                lhs =
                    Expr { id, kind: ExprKind::Unary { op: un_op, operand: Box::new(lhs) }, span };
            }
            // `f(args...)` — function call. Arguments are
            // *assignment-expressions* per §6.5.2.2, so the comma
            // between them is a separator, not the comma operator.
            // We call `parse_expr_bp(p, 0)` for each argument because
            // the Pratt loop does not currently fold comma (that's
            // task 05-12); once it does, this call site must bump to
            // the assignment binding-power floor — grep for
            // "05-12 follow-up" when that task lands.
            Punct::LParen => {
                let lparen_span = op_span;
                p.bump();
                let (args, rparen_span) = parse_call_args(p, lparen_span);
                let span = lhs.span.to(rparen_span);
                let id = p.fresh_id();
                lhs = Expr { id, kind: ExprKind::Call { callee: Box::new(lhs), args }, span };
            }
            // Any other punctuator is outside this production —
            // defer to the caller (usually the Pratt infix loop).
            _ => break,
        }
    }
    Some(lhs)
}

/// Consume the identifier that must follow `.` or `->` in a
/// postfix member access. Emits a diagnostic and returns `None`
/// when the next token is not an identifier; the cursor is left on
/// the offending token so the `parse_postfix` loop can cleanly
/// break without spinning.
///
/// C99 §6.5.2.3 requires the field selector to be an *identifier*,
/// specifically the name of a struct/union member — but the member
/// list isn't resolved until HIR-lowering, so all the parser can
/// check here is the token shape. A keyword like `.if` or a
/// literal like `.42` is rejected at this layer.
fn expect_member_ident(
    p: &mut Parser<'_>,
    op_span: rcc_span::Span,
    op_spelling: &str,
) -> Option<(Symbol, rcc_span::Span)> {
    match p.peek() {
        Some(t) => {
            if let TokenKind::Ident(sym) = t.kind {
                let span = t.span;
                p.bump();
                Some((sym, span))
            } else {
                let at = t.span;
                p.session
                    .handler
                    .struct_err(at, format!("expected identifier after `{op_spelling}`"))
                    .label(op_span, "member access here")
                    .emit();
                None
            }
        }
        None => {
            p.session
                .handler
                .struct_err(op_span, format!("expected identifier after `{op_spelling}`"))
                .emit();
            None
        }
    }
}

/// Parse the argument list of a function call, starting **after**
/// the opening `(`. Returns the (possibly empty) argument vector
/// and the span of the closing `)` (or a best-effort fallback span
/// on recovery).
///
/// Shape:
///
/// ```text
/// argument-expression-list:
///     assignment-expression
///     argument-expression-list , assignment-expression
/// ```
///
/// Empty `()` is legal (C99 §6.5.2.2 ¶2 — it just means no
/// arguments were supplied). A trailing `,` is a syntax error: it
/// is diagnosed but the already-collected prefix of arguments is
/// still returned so higher-level callers can make progress.
///
/// We read arguments with `parse_expr_bp(p, 0)` because comma is
/// not a Pratt-folded operator yet (task 05-12). Once it is, the
/// floor must rise to the assignment binding power so that a bare
/// `,` at this layer stays a separator; see the comment in
/// [`parse_postfix`] for the grep-able marker.
fn parse_call_args(p: &mut Parser<'_>, lparen_span: rcc_span::Span) -> (Vec<Expr>, rcc_span::Span) {
    let mut args = Vec::new();
    // `last_span` tracks the best real-source span we can fall back
    // to when recovery hits end-of-input. `p.cur_span()` past EOI
    // returns `DUMMY_SP` (a sentinel `FileId`) which would panic on
    // `.to()` against `lhs.span` in the caller, so we never return
    // it from this function. Starting at `lparen_span` guarantees
    // every recovery path hands back a span from the right file.
    let mut last_span = lparen_span;
    // Fast path: immediate `)` → empty argument list.
    if let Some(t) = p.peek() {
        if matches!(t.kind, TokenKind::Punct(Punct::RParen)) {
            let rparen_span = t.span;
            p.bump();
            return (args, rparen_span);
        }
    }
    loop {
        let Some(arg) = parse_expr_bp(p, 0) else {
            return (args, last_span);
        };
        last_span = arg.span;
        args.push(arg);
        match p.peek() {
            Some(t) if matches!(t.kind, TokenKind::Punct(Punct::Comma)) => {
                last_span = t.span;
                p.bump();
                // Reject `f(a,)` — a trailing comma is not C99
                // grammar. Diagnose but still close the call if `)`
                // follows, so the prefix arguments survive.
                let next_rparen = p
                    .peek()
                    .filter(|t2| matches!(t2.kind, TokenKind::Punct(Punct::RParen)))
                    .map(|t2| t2.span);
                if let Some(rparen_span) = next_rparen {
                    p.session
                        .handler
                        .struct_err(rparen_span, "expected expression after `,` in argument list")
                        .label(lparen_span, "in this call")
                        .emit();
                    p.bump();
                    return (args, rparen_span);
                }
                continue;
            }
            Some(t) if matches!(t.kind, TokenKind::Punct(Punct::RParen)) => {
                let rparen_span = t.span;
                p.bump();
                return (args, rparen_span);
            }
            Some(t) => {
                let at = t.span;
                p.session
                    .handler
                    .struct_err(at, "expected `,` or `)` in argument list")
                    .label(lparen_span, "in this call")
                    .emit();
                return (args, last_span);
            }
            None => {
                // End-of-input with no matching `)`. Diagnose at
                // `lparen_span` so the label points inside a real
                // source file (the EOI `cur_span` is `DUMMY_SP`).
                p.session
                    .handler
                    .struct_err(lparen_span, "unclosed argument list: expected `)`")
                    .emit();
                return (args, last_span);
            }
        }
    }
}

/// Top-level C99 expression entry point.
///
/// Drives a Pratt / precedence-climbing loop starting at the lowest
/// binding power, so every binary and assignment operator in the
/// table documented at the module level is accepted. The comma
/// operator (§6.5.17) is *not* folded here — that's task 05-12 —
/// so the current shape corresponds to the *assignment-expression*
/// non-terminal, which is exactly what most C grammar productions
/// (function arguments, array sizes, initializer expressions, etc.)
/// spell out anyway.
///
/// Returns `None` when no primary expression is available at the
/// cursor position; in that case a diagnostic has already been
/// emitted by [`parse_primary`] and the cursor is left where the
/// error happened so the caller can decide how to recover.
pub fn parse_expression(p: &mut Parser<'_>) -> Option<Expr> {
    parse_expr_bp(p, 0)
}

/// Parse an expression whose top-level operator has a left binding
/// power of at least `min_bp`. This is the engine behind
/// [`parse_expression`] and is exposed so later tasks (05-09 unary
/// prefix, 05-10 cast, 05-11 conditional) can weave additional
/// grammar layers in without duplicating the precedence table.
///
/// The control flow is textbook Matklad-style precedence climbing:
///
/// 1. Consume a primary / unary expression as the initial LHS.
/// 2. Peek at the next token. If it isn't a known infix operator,
///    or its `l_bp` is strictly below `min_bp`, stop and return the
///    LHS — the caller reduces it against its own frame.
/// 3. Otherwise consume the operator, recurse with `min_bp = r_bp`
///    to collect the RHS, fold into the appropriate
///    [`ExprKind::Binary`] / [`ExprKind::Assign`] node, and loop.
///
/// A failed RHS parse (e.g. garbage after a `+`) aborts the loop
/// early and returns the LHS already collected — [`parse_primary`]
/// has emitted a diagnostic at the offending position and the
/// cursor has been left there, so higher layers can still resync.
/// We deliberately avoid fabricating a dummy RHS because that would
/// invent a span and a `NodeId` that no source text backs.
pub fn parse_expr_bp(p: &mut Parser<'_>, min_bp: u8) -> Option<Expr> {
    // Task 05-09 wedge: the Pratt leaf is now `parse_prefix_unary`,
    // which runs its own left-to-right prefix-operator chain before
    // falling back to `parse_postfix`, which in turn consumes the
    // primary and any trailing `[...]`, `.`, `->`, `++`, `--`, or
    // `( args )` postfix trailers. This keeps all three layers —
    // prefix unary, postfix trailers, and binary / assignment infix
    // — in their C99 §6.5 precedence order without any bp numbering
    // for the unary levels themselves (their relative strength is
    // encoded by *where* in the call graph they live).
    let mut lhs = parse_prefix_unary(p)?;
    while let Some(op) = peek_infix(p) {
        let (l_bp, r_bp) = infix_bp(op);
        if l_bp < min_bp {
            break;
        }
        // Commit to this operator: consume it and recurse on the RHS.
        p.bump();
        let Some(rhs) = parse_expr_bp(p, r_bp) else {
            // `parse_primary` already diagnosed the missing RHS;
            // returning the partially-built LHS preserves as much of
            // the user's tree as possible for downstream tasks.
            break;
        };
        let span = lhs.span.to(rhs.span);
        let id = p.fresh_id();
        lhs = match op {
            InfixOp::Binary(bin) => Expr {
                id,
                kind: ExprKind::Binary { op: bin, lhs: Box::new(lhs), rhs: Box::new(rhs) },
                span,
            },
            InfixOp::Assign(aop) => Expr {
                id,
                kind: ExprKind::Assign { op: aop, lhs: Box::new(lhs), rhs: Box::new(rhs) },
                span,
            },
        };
    }
    Some(lhs)
}

/// Infix-operator discriminant consumed by [`parse_expr_bp`].
///
/// We wrap the two AST flavours (plain binary vs assignment) because
/// they build into different `ExprKind` variants but share the same
/// Pratt machinery — keeping the dispatch in the shape of the op
/// rather than on its precedence number leaves the table readable
/// and `match`-exhaustive.
#[derive(Copy, Clone, Debug)]
enum InfixOp {
    Binary(BinOp),
    Assign(AssignOp),
}

/// Peek at the current token and, if it is a binary or assignment
/// operator, return the corresponding [`InfixOp`] *without* advancing
/// the cursor. Returning `None` is the signal to [`parse_expr_bp`]
/// that the expression has ended (or that the token belongs to a
/// surrounding construct like `)`, `;`, `,`, `?`, or `:`).
fn peek_infix(p: &Parser<'_>) -> Option<InfixOp> {
    let TokenKind::Punct(punct) = p.peek()?.kind else {
        return None;
    };
    Some(match punct {
        // Arithmetic.
        Punct::Plus => InfixOp::Binary(BinOp::Add),
        Punct::Minus => InfixOp::Binary(BinOp::Sub),
        Punct::Star => InfixOp::Binary(BinOp::Mul),
        Punct::Slash => InfixOp::Binary(BinOp::Div),
        Punct::Percent => InfixOp::Binary(BinOp::Rem),
        // Shifts.
        Punct::ShlShl => InfixOp::Binary(BinOp::Shl),
        Punct::ShrShr => InfixOp::Binary(BinOp::Shr),
        // Relational / equality.
        Punct::Lt => InfixOp::Binary(BinOp::Lt),
        Punct::Le => InfixOp::Binary(BinOp::Le),
        Punct::Gt => InfixOp::Binary(BinOp::Gt),
        Punct::Ge => InfixOp::Binary(BinOp::Ge),
        Punct::EqEq => InfixOp::Binary(BinOp::Eq),
        Punct::BangEq => InfixOp::Binary(BinOp::Ne),
        // Bitwise.
        Punct::Amp => InfixOp::Binary(BinOp::BitAnd),
        Punct::Caret => InfixOp::Binary(BinOp::BitXor),
        Punct::Pipe => InfixOp::Binary(BinOp::BitOr),
        // Logical.
        Punct::AmpAmp => InfixOp::Binary(BinOp::LogAnd),
        Punct::PipePipe => InfixOp::Binary(BinOp::LogOr),
        // Assignments (right-associative family, §6.5.16).
        Punct::Eq => InfixOp::Assign(AssignOp::Eq),
        Punct::PlusEq => InfixOp::Assign(AssignOp::AddEq),
        Punct::MinusEq => InfixOp::Assign(AssignOp::SubEq),
        Punct::StarEq => InfixOp::Assign(AssignOp::MulEq),
        Punct::SlashEq => InfixOp::Assign(AssignOp::DivEq),
        Punct::PercentEq => InfixOp::Assign(AssignOp::RemEq),
        Punct::ShlEq => InfixOp::Assign(AssignOp::ShlEq),
        Punct::ShrEq => InfixOp::Assign(AssignOp::ShrEq),
        Punct::AmpEq => InfixOp::Assign(AssignOp::AndEq),
        Punct::CaretEq => InfixOp::Assign(AssignOp::XorEq),
        Punct::PipeEq => InfixOp::Assign(AssignOp::OrEq),
        // Everything else — including `,`, `?`, `:`, and all brackets
        // / delimiters — is NOT an infix operator at this layer.
        _ => return None,
    })
}

/// Binding-power table for every C99 §6.5 infix operator handled by
/// this task. See the module-level docs for the full associativity
/// rationale and the Matklad `(l_bp, r_bp)` encoding.
///
/// All numbers are even/odd pairs so the difference between a left-
/// associative `(n, n+1)` and a right-associative `(n+1, n)` is the
/// *single* bit that flips recursion behaviour — this is what makes
/// `a = b = c` parse as `a = (b = c)` while `a + b + c` parses as
/// `(a + b) + c`.
fn infix_bp(op: InfixOp) -> (u8, u8) {
    match op {
        // Level 1: assignment — right-associative, lowest in §6.5.
        InfixOp::Assign(_) => (2, 1),
        // Level 2 … 10: binary operators in ascending C99 precedence.
        InfixOp::Binary(bin) => match bin {
            BinOp::LogOr => (3, 4),
            BinOp::LogAnd => (5, 6),
            BinOp::BitOr => (7, 8),
            BinOp::BitXor => (9, 10),
            BinOp::BitAnd => (11, 12),
            BinOp::Eq | BinOp::Ne => (13, 14),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => (15, 16),
            BinOp::Shl | BinOp::Shr => (17, 18),
            BinOp::Add | BinOp::Sub => (19, 20),
            BinOp::Mul | BinOp::Div | BinOp::Rem => (21, 22),
        },
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

    // ── 05-08 Pratt precedence / associativity ──────────────────────

    /// Build a pp-token stream for `src` where every character is an
    /// identifier, a punctuator, or a single-digit integer. This is
    /// just enough surface area to write expression Pratt tests
    /// without replicating the lexer — it keeps the acceptance tests
    /// readable in source form rather than as long arrays of
    /// `PpTokenKind` literals.
    fn lex_ascii(fid: FileId, src: &str) -> Vec<PpToken> {
        let mut out = Vec::new();
        let bytes = src.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let lo = i as u32;
            let b = bytes[i];
            // Skip ASCII whitespace — not emitted into the stream.
            if b == b' ' || b == b'\t' || b == b'\n' {
                i += 1;
                continue;
            }
            // Single-letter identifier.
            if b.is_ascii_alphabetic() || b == b'_' {
                i += 1;
                out.push(pp(PpTokenKind::Ident, fid, lo, i as u32));
                continue;
            }
            // Single-digit integer.
            if b.is_ascii_digit() {
                i += 1;
                out.push(pp(PpTokenKind::PpNumber(PpNumberKind::Integer), fid, lo, i as u32));
                continue;
            }
            // Two- and three-char punctuators, longest match first.
            let rest = &bytes[i..];
            let (punct, len) = if rest.starts_with(b"<<=") {
                (Punct::ShlEq, 3)
            } else if rest.starts_with(b">>=") {
                (Punct::ShrEq, 3)
            } else if rest.starts_with(b"<<") {
                (Punct::ShlShl, 2)
            } else if rest.starts_with(b">>") {
                (Punct::ShrShr, 2)
            } else if rest.starts_with(b"<=") {
                (Punct::Le, 2)
            } else if rest.starts_with(b">=") {
                (Punct::Ge, 2)
            } else if rest.starts_with(b"==") {
                (Punct::EqEq, 2)
            } else if rest.starts_with(b"!=") {
                (Punct::BangEq, 2)
            } else if rest.starts_with(b"&&") {
                (Punct::AmpAmp, 2)
            } else if rest.starts_with(b"||") {
                (Punct::PipePipe, 2)
            } else if rest.starts_with(b"++") {
                (Punct::PlusPlus, 2)
            } else if rest.starts_with(b"--") {
                (Punct::MinusMinus, 2)
            } else if rest.starts_with(b"->") {
                (Punct::Arrow, 2)
            } else if rest.starts_with(b"+=") {
                (Punct::PlusEq, 2)
            } else if rest.starts_with(b"-=") {
                (Punct::MinusEq, 2)
            } else if rest.starts_with(b"*=") {
                (Punct::StarEq, 2)
            } else if rest.starts_with(b"/=") {
                (Punct::SlashEq, 2)
            } else if rest.starts_with(b"%=") {
                (Punct::PercentEq, 2)
            } else if rest.starts_with(b"&=") {
                (Punct::AmpEq, 2)
            } else if rest.starts_with(b"^=") {
                (Punct::CaretEq, 2)
            } else if rest.starts_with(b"|=") {
                (Punct::PipeEq, 2)
            } else {
                let single = match b {
                    b'+' => Punct::Plus,
                    b'-' => Punct::Minus,
                    b'*' => Punct::Star,
                    b'/' => Punct::Slash,
                    b'%' => Punct::Percent,
                    b'<' => Punct::Lt,
                    b'>' => Punct::Gt,
                    b'&' => Punct::Amp,
                    b'^' => Punct::Caret,
                    b'|' => Punct::Pipe,
                    b'=' => Punct::Eq,
                    b'(' => Punct::LParen,
                    b')' => Punct::RParen,
                    b'[' => Punct::LBracket,
                    b']' => Punct::RBracket,
                    b',' => Punct::Comma,
                    b'.' => Punct::Dot,
                    b'~' => Punct::Tilde,
                    b'!' => Punct::Bang,
                    other => panic!("lex_ascii: unsupported byte {:?}", other as char),
                };
                (single, 1)
            };
            i += len;
            out.push(pp(PpTokenKind::Punct(punct), fid, lo, i as u32));
        }
        out
    }

    /// Helper: feed `src` through the mini-lexer and parse it as a
    /// top-level expression. Panics on parse failure because these
    /// tests all feed well-formed input.
    fn parse_expr_str(src: &str) -> (Expr, rcc_errors::CaptureEmitter) {
        let (mut sess, fid, cap) = mk_session(src);
        let pps = lex_ascii(fid, src);
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_expression(&mut parser).expect("expression parses");
        assert_eq!(
            parser.cursor,
            parser.tokens.len(),
            "Pratt parser must consume every token of {src:?}",
        );
        (e, cap)
    }

    /// Assert that `e` is `Binary { op, lhs = <Ident lsym>, rhs = <Ident rsym> }`.
    fn assert_bin_ident(
        e: &Expr,
        expected_op: BinOp,
        lsym: &str,
        rsym: &str,
        interner: &rcc_span::Interner,
    ) {
        match &e.kind {
            ExprKind::Binary { op, lhs, rhs } => {
                assert_eq!(*op, expected_op, "op mismatch");
                match &lhs.kind {
                    ExprKind::Ident(s) => assert_eq!(interner.get(*s), lsym),
                    other => panic!("lhs must be Ident({lsym}), got {other:?}"),
                }
                match &rhs.kind {
                    ExprKind::Ident(s) => assert_eq!(interner.get(*s), rsym),
                    other => panic!("rhs must be Ident({rsym}), got {other:?}"),
                }
            }
            other => panic!("expected Binary, got {other:?}"),
        }
    }

    #[test]
    fn multiplicative_binds_tighter_than_additive() {
        // Acceptance: `a + b * c` parses as `a + (b * c)` per C99
        // §6.5.5/§6.5.6.
        let src = "a + b * c";
        let (e, cap) = parse_expr_str(src);
        assert!(cap.diagnostics().is_empty(), "valid expr must be diag-free");
        match e.kind {
            ExprKind::Binary { op: BinOp::Add, lhs, rhs } => {
                match lhs.kind {
                    ExprKind::Ident(_) => {}
                    other => panic!("expected `a` on lhs, got {other:?}"),
                }
                match rhs.kind {
                    ExprKind::Binary { op: BinOp::Mul, lhs: inner_l, rhs: inner_r } => {
                        assert!(matches!(inner_l.kind, ExprKind::Ident(_)));
                        assert!(matches!(inner_r.kind, ExprKind::Ident(_)));
                    }
                    other => panic!("expected `b * c` on rhs, got {other:?}"),
                }
            }
            other => panic!("expected top-level `+`, got {other:?}"),
        }
    }

    #[test]
    fn assignment_is_right_associative() {
        // Acceptance: `a = b = c` parses as `a = (b = c)` per §6.5.16.
        let (e, _cap) = parse_expr_str("a = b = c");
        match e.kind {
            ExprKind::Assign { op: AssignOp::Eq, lhs, rhs } => {
                assert!(matches!(lhs.kind, ExprKind::Ident(_)), "outer lhs must be `a`");
                match rhs.kind {
                    ExprKind::Assign { op: AssignOp::Eq, lhs: inner_l, rhs: inner_r } => {
                        assert!(matches!(inner_l.kind, ExprKind::Ident(_)));
                        assert!(matches!(inner_r.kind, ExprKind::Ident(_)));
                    }
                    other => panic!("inner rhs must be `b = c`, got {other:?}"),
                }
            }
            other => panic!("expected top-level assignment, got {other:?}"),
        }
    }

    #[test]
    fn equality_is_left_associative() {
        // Acceptance: `a == b != c` parses as `(a == b) != c`.
        let (e, _cap) = parse_expr_str("a == b != c");
        match e.kind {
            ExprKind::Binary { op: BinOp::Ne, lhs, rhs } => {
                match lhs.kind {
                    ExprKind::Binary { op: BinOp::Eq, lhs: ll, rhs: lr } => {
                        assert!(matches!(ll.kind, ExprKind::Ident(_)));
                        assert!(matches!(lr.kind, ExprKind::Ident(_)));
                    }
                    other => panic!("outer lhs must be `a == b`, got {other:?}"),
                }
                assert!(matches!(rhs.kind, ExprKind::Ident(_)), "outer rhs must be `c`");
            }
            other => panic!("expected top-level `!=`, got {other:?}"),
        }
    }

    #[test]
    fn mixed_precedence_shift_additive_multiplicative() {
        // `a + b << c * d` parses as `(a + b) << (c * d)` because
        // `*` > `+` > `<<` in C99 §6.5 precedence.
        let (e, _cap) = parse_expr_str("a + b << c * d");
        match e.kind {
            ExprKind::Binary { op: BinOp::Shl, lhs, rhs } => {
                match lhs.kind {
                    ExprKind::Binary { op: BinOp::Add, .. } => {}
                    other => panic!("shl-lhs must be `a + b`, got {other:?}"),
                }
                match rhs.kind {
                    ExprKind::Binary { op: BinOp::Mul, .. } => {}
                    other => panic!("shl-rhs must be `c * d`, got {other:?}"),
                }
            }
            other => panic!("expected top-level `<<`, got {other:?}"),
        }
    }

    #[test]
    fn assignment_rhs_folds_arithmetic_inside_it() {
        // §6.5.16: the RHS of `=` is itself an assignment-expression
        // — which means every tighter operator (arithmetic, shifts,
        // etc.) reduces inside it. `a = b + c * d` must tree as
        // `a = (b + (c * d))`.
        let (e, _cap) = parse_expr_str("a = b + c * d");
        match e.kind {
            ExprKind::Assign { op: AssignOp::Eq, lhs, rhs } => {
                assert!(matches!(lhs.kind, ExprKind::Ident(_)));
                match rhs.kind {
                    ExprKind::Binary { op: BinOp::Add, lhs: add_l, rhs: add_r } => {
                        assert!(matches!(add_l.kind, ExprKind::Ident(_)));
                        match add_r.kind {
                            ExprKind::Binary { op: BinOp::Mul, .. } => {}
                            other => panic!("inner rhs must be `c * d`, got {other:?}"),
                        }
                    }
                    other => panic!("rhs must be `b + (c*d)`, got {other:?}"),
                }
            }
            other => panic!("expected top-level `=`, got {other:?}"),
        }
    }

    #[test]
    fn compound_assignment_eg_plus_eq_is_right_associative() {
        // `a += b *= c` parses as `a += (b *= c)` — the whole
        // assignment family shares the same right-associative slot.
        let (e, _cap) = parse_expr_str("a += b *= c");
        match e.kind {
            ExprKind::Assign { op: AssignOp::AddEq, lhs, rhs } => {
                assert!(matches!(lhs.kind, ExprKind::Ident(_)));
                match rhs.kind {
                    ExprKind::Assign { op: AssignOp::MulEq, .. } => {}
                    other => panic!("inner rhs must be `b *= c`, got {other:?}"),
                }
            }
            other => panic!("expected top-level `+=`, got {other:?}"),
        }
    }

    #[test]
    fn logical_and_beats_logical_or() {
        // `a || b && c` parses as `a || (b && c)` (§6.5.13 vs §6.5.14).
        let (e, _cap) = parse_expr_str("a || b && c");
        match e.kind {
            ExprKind::Binary { op: BinOp::LogOr, lhs, rhs } => {
                assert!(matches!(lhs.kind, ExprKind::Ident(_)));
                match rhs.kind {
                    ExprKind::Binary { op: BinOp::LogAnd, .. } => {}
                    other => panic!("inner rhs must be `b && c`, got {other:?}"),
                }
            }
            other => panic!("expected top-level `||`, got {other:?}"),
        }
    }

    #[test]
    fn bitwise_precedence_matches_c99() {
        // C99 orders bitwise from tightest to loosest: `&` > `^` > `|`.
        // `a | b ^ c & d` therefore parses as `a | (b ^ (c & d))`.
        let (e, _cap) = parse_expr_str("a | b ^ c & d");
        match e.kind {
            ExprKind::Binary { op: BinOp::BitOr, lhs, rhs } => {
                assert!(matches!(lhs.kind, ExprKind::Ident(_)));
                match rhs.kind {
                    ExprKind::Binary { op: BinOp::BitXor, lhs: xor_l, rhs: xor_r } => {
                        assert!(matches!(xor_l.kind, ExprKind::Ident(_)));
                        match xor_r.kind {
                            ExprKind::Binary { op: BinOp::BitAnd, .. } => {}
                            other => panic!("inner must be `c & d`, got {other:?}"),
                        }
                    }
                    other => panic!("rhs must be `b ^ (c & d)`, got {other:?}"),
                }
            }
            other => panic!("expected top-level `|`, got {other:?}"),
        }
    }

    #[test]
    fn paren_delegates_to_full_expression_parser() {
        // Regression against the old primary-only stub: the inner
        // production must now reduce a full assignment expression,
        // so `(a + b)` yields a `Paren` wrapping a `Binary(Add, …)`.
        let (e, _cap) = parse_expr_str("(a + b)");
        match e.kind {
            ExprKind::Paren(inner) => match inner.kind {
                ExprKind::Binary { op: BinOp::Add, .. } => {}
                other => panic!("inner must be `a + b`, got {other:?}"),
            },
            other => panic!("expected Paren, got {other:?}"),
        }
    }

    #[test]
    fn left_associative_chain_32_deep_does_not_stack_overflow() {
        // Deep nesting acceptance: a 64-long `1 + 1 + 1 + ...` chain
        // must parse without blowing the default Rust stack. Each
        // `+` adds exactly one recursive Pratt frame, so 64 frames is
        // still well below `RUST_MIN_STACK`.
        let n = 64;
        let src: String =
            std::iter::once("1".to_owned()).chain((0..n).map(|_| " + 1".to_owned())).collect();
        let (e, _cap) = parse_expr_str(&src);
        // Count the left-leaning Add spine — for N trailing `+ 1`s
        // we must have exactly N Add nodes, the leftmost of which
        // wraps an `IntLit`.
        let mut depth = 0;
        let mut cur = e;
        while let ExprKind::Binary { op: BinOp::Add, lhs, .. } = cur.kind {
            depth += 1;
            cur = *lhs;
        }
        assert_eq!(depth, n, "expected {n} left-leaning Add nodes, got {depth}");
        assert!(matches!(cur.kind, ExprKind::IntLit { .. }));
    }

    #[test]
    fn paren_nesting_32_deep_does_not_stack_overflow() {
        // `((((...((a))...))))` with 40 parens on each side — each
        // paren layer is one primary recursion + one Pratt frame, so
        // 40 layers stays well under the default stack.
        let depth = 40;
        let mut src = String::new();
        for _ in 0..depth {
            src.push('(');
        }
        src.push('a');
        for _ in 0..depth {
            src.push(')');
        }
        let (mut e, _cap) = parse_expr_str(&src);
        for _ in 0..depth {
            match e.kind {
                ExprKind::Paren(inner) => e = *inner,
                other => panic!("expected Paren wrapper, got {other:?}"),
            }
        }
        assert!(matches!(e.kind, ExprKind::Ident(_)));
    }

    #[test]
    fn right_associative_assignment_chain_is_deeply_nested() {
        // `a = a = a = ... = a` (32 `=`s) — right-associative so the
        // AST leans rightward; each `=` adds one Pratt frame.
        let n = 32;
        let mut src = String::new();
        for _ in 0..n {
            src.push_str("a = ");
        }
        src.push('a');
        let (mut e, _cap) = parse_expr_str(&src);
        let mut seen = 0;
        while let ExprKind::Assign { op: AssignOp::Eq, rhs, .. } = e.kind {
            seen += 1;
            e = *rhs;
        }
        assert_eq!(seen, n, "expected {n} right-leaning `=` nodes, got {seen}");
        assert!(matches!(e.kind, ExprKind::Ident(_)));
    }

    #[test]
    fn binary_span_covers_full_operand_range() {
        // The folded node's span must stretch from the LHS start to
        // the RHS end so that later diagnostics can underline the
        // whole sub-expression cleanly.
        let src = "a + b";
        let (e, _cap) = parse_expr_str(src);
        assert_eq!(e.span.lo.0, 0, "span must start at `a`");
        assert_eq!(e.span.hi.0, 5, "span must end at the end of `b`");
    }

    /// Suppress `unused` for the ident-lookup helper above: it's
    /// currently only exercised by this follow-up test. Keeping it
    /// as a helper instead of inlining makes the downstream unary /
    /// postfix tests easier to add without rewriting everything.
    #[test]
    fn ident_helper_smoke() {
        let (mut sess, fid, _cap) = mk_session("a - b");
        let pps = lex_ascii(fid, "a - b");
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_expression(&mut parser).expect("a - b parses");
        let interner = &parser.session.interner;
        assert_bin_ident(&e, BinOp::Sub, "a", "b", interner);
    }

    // ── 05-09 postfix / unary ───────────────────────────────────────

    /// Look up the spelling of an identifier behind a `Symbol`.
    fn ident_str<'a>(e: &Expr, interner: &'a rcc_span::Interner) -> &'a str {
        match e.kind {
            ExprKind::Ident(s) => interner.get(s),
            _ => panic!("expected Ident, got {:?}", e.kind),
        }
    }

    #[test]
    fn postfix_chain_is_left_to_right() {
        // Acceptance: `a.b->c[0]++` parses as
        // `PostInc(Index(Arrow(Member(a, b), c), 0))` — the §6.5.2
        // postfix grammar is left-recursive, so every trailer wraps
        // the whole chain to its left.
        let src = "a.b->c[0]++";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = lex_ascii(fid, src);
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_expression(&mut parser).expect("postfix chain parses");
        assert!(cap.diagnostics().is_empty(), "valid chain must be diag-free");
        let interner = &parser.session.interner;
        // Outermost: PostInc.
        let inner = match e.kind {
            ExprKind::Unary { op: UnOp::PostInc, operand } => *operand,
            other => panic!("outer must be PostInc, got {other:?}"),
        };
        // Next: Index[.., 0].
        let (idx_base, idx_index) = match inner.kind {
            ExprKind::Index { base, index } => (*base, *index),
            other => panic!("level 2 must be Index, got {other:?}"),
        };
        assert!(matches!(idx_index.kind, ExprKind::IntLit { .. }), "index must be `0`");
        // Next: Arrow(.., c).
        let (arr_base, arr_field) = match idx_base.kind {
            ExprKind::Arrow { base, field } => (*base, field),
            other => panic!("level 3 must be Arrow, got {other:?}"),
        };
        assert_eq!(interner.get(arr_field), "c");
        // Next: Member(a, b).
        match arr_base.kind {
            ExprKind::Member { base, field } => {
                assert_eq!(interner.get(field), "b");
                assert_eq!(ident_str(&base, interner), "a");
            }
            other => panic!("level 4 must be Member, got {other:?}"),
        }
        // Span covers the whole input.
        assert_eq!(e.span.lo.0, 0);
        assert_eq!(e.span.hi.0, src.len() as u32);
    }

    #[test]
    fn prefix_chain_addrof_deref_addrof() {
        // Acceptance: `&*&x` parses as `AddrOf(Deref(AddrOf(x)))` —
        // three nested `Unary` nodes, outermost is the first `&`.
        let src = "&*&x";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = lex_ascii(fid, src);
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_expression(&mut parser).expect("&*&x parses");
        assert!(cap.diagnostics().is_empty());
        let interner = &parser.session.interner;
        let inner = match e.kind {
            ExprKind::Unary { op: UnOp::AddrOf, operand } => *operand,
            other => panic!("outer must be AddrOf, got {other:?}"),
        };
        let inner = match inner.kind {
            ExprKind::Unary { op: UnOp::Deref, operand } => *operand,
            other => panic!("level 2 must be Deref, got {other:?}"),
        };
        let inner = match inner.kind {
            ExprKind::Unary { op: UnOp::AddrOf, operand } => *operand,
            other => panic!("level 3 must be AddrOf, got {other:?}"),
        };
        assert_eq!(ident_str(&inner, interner), "x");
    }

    #[test]
    fn call_with_two_args_parses() {
        // `f(a, b)` → Call { callee = f, args = [a, b] }.
        let src = "f(a, b)";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = lex_ascii(fid, src);
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_expression(&mut parser).expect("f(a, b) parses");
        assert!(cap.diagnostics().is_empty());
        let interner = &parser.session.interner;
        match e.kind {
            ExprKind::Call { callee, args } => {
                assert_eq!(ident_str(&callee, interner), "f");
                assert_eq!(args.len(), 2);
                assert_eq!(ident_str(&args[0], interner), "a");
                assert_eq!(ident_str(&args[1], interner), "b");
            }
            other => panic!("expected Call, got {other:?}"),
        }
        assert_eq!(e.span.lo.0, 0);
        assert_eq!(e.span.hi.0, src.len() as u32);
    }

    #[test]
    fn empty_call_parses_with_zero_args() {
        // `f()` is legal per §6.5.2.2 ¶2 — zero-argument call.
        let src = "f()";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = lex_ascii(fid, src);
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_expression(&mut parser).expect("f() parses");
        assert!(cap.diagnostics().is_empty());
        let interner = &parser.session.interner;
        match e.kind {
            ExprKind::Call { callee, args } => {
                assert_eq!(ident_str(&callee, interner), "f");
                assert!(args.is_empty());
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn call_arg_is_assignment_expression_not_comma_folded() {
        // `f(a = b, c)` — the `=` must fold inside argument 1, NOT
        // make the argument list a single comma-expression. This
        // pins down the "arguments are assignment-expressions"
        // decision so the follow-up task 05-12 (comma operator)
        // doesn't silently regress it.
        let src = "f(a = b, c)";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = lex_ascii(fid, src);
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_expression(&mut parser).expect("f(a = b, c) parses");
        assert!(cap.diagnostics().is_empty());
        match e.kind {
            ExprKind::Call { args, .. } => {
                assert_eq!(args.len(), 2, "must be two arguments, not one comma-expr");
                assert!(
                    matches!(args[0].kind, ExprKind::Assign { op: AssignOp::Eq, .. }),
                    "arg 0 must be `a = b`",
                );
                assert!(matches!(args[1].kind, ExprKind::Ident(_)));
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn logical_not_on_deref_parses_as_not_of_deref() {
        // Acceptance: `!*p` is `LogNot(Deref(p))` — prefix chain
        // associates right-to-left because `parse_prefix_unary`
        // recurses on itself.
        let src = "!*p";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = lex_ascii(fid, src);
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_expression(&mut parser).expect("!*p parses");
        let interner = &parser.session.interner;
        let inner = match e.kind {
            ExprKind::Unary { op: UnOp::LogNot, operand } => *operand,
            other => panic!("outer must be LogNot, got {other:?}"),
        };
        let inner = match inner.kind {
            ExprKind::Unary { op: UnOp::Deref, operand } => *operand,
            other => panic!("inner must be Deref, got {other:?}"),
        };
        assert_eq!(ident_str(&inner, interner), "p");
    }

    #[test]
    fn preinc_wraps_postdec() {
        // `++a--` parses as `PreInc(PostDec(a))` because
        // `parse_prefix_unary` recurses into another unary-
        // expression (which resolves to the postfix chain `a--`)
        // before wrapping with the prefix `++`. Semantically the
        // inner `a--` is an rvalue that `++` cannot modify, but
        // that's an lvalue check for typeck (§6.5.3.1 ¶1), not a
        // grammar rejection — matches clang/gcc/chibicc behaviour.
        let src = "++a--";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = lex_ascii(fid, src);
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_expression(&mut parser).expect("++a-- parses");
        assert!(cap.diagnostics().is_empty(), "grammar-level clean");
        let interner = &parser.session.interner;
        let inner = match e.kind {
            ExprKind::Unary { op: UnOp::PreInc, operand } => *operand,
            other => panic!("outer must be PreInc, got {other:?}"),
        };
        let inner = match inner.kind {
            ExprKind::Unary { op: UnOp::PostDec, operand } => *operand,
            other => panic!("inner must be PostDec, got {other:?}"),
        };
        assert_eq!(ident_str(&inner, interner), "a");
    }

    #[test]
    fn call_chained_with_index_member_arrow_matches_spec() {
        // Deliverable: `f(a)[b].c->d++` — a chain that mixes every
        // postfix trailer kind. Expected shape (outer-in):
        //   PostInc(Arrow(Member(Index(Call(f,[a]), b), c), d)).
        let src = "f(a)[b].c->d++";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = lex_ascii(fid, src);
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_expression(&mut parser).expect("chain parses");
        assert!(cap.diagnostics().is_empty());
        let interner = &parser.session.interner;
        // PostInc ▶ Arrow(_, d) ▶ Member(_, c) ▶ Index(_, b) ▶ Call(f, [a])
        let l1 = match e.kind {
            ExprKind::Unary { op: UnOp::PostInc, operand } => *operand,
            other => panic!("outer must be PostInc, got {other:?}"),
        };
        let l2 = match l1.kind {
            ExprKind::Arrow { base, field } => {
                assert_eq!(interner.get(field), "d");
                *base
            }
            other => panic!("level 2 must be Arrow, got {other:?}"),
        };
        let l3 = match l2.kind {
            ExprKind::Member { base, field } => {
                assert_eq!(interner.get(field), "c");
                *base
            }
            other => panic!("level 3 must be Member, got {other:?}"),
        };
        let l4 = match l3.kind {
            ExprKind::Index { base, index } => {
                assert_eq!(ident_str(&index, interner), "b");
                *base
            }
            other => panic!("level 4 must be Index, got {other:?}"),
        };
        match l4.kind {
            ExprKind::Call { callee, args } => {
                assert_eq!(ident_str(&callee, interner), "f");
                assert_eq!(args.len(), 1);
                assert_eq!(ident_str(&args[0], interner), "a");
            }
            other => panic!("level 5 must be Call, got {other:?}"),
        }
    }

    #[test]
    fn prefix_is_looser_than_postfix() {
        // `-a++` parses as `Neg(PostInc(a))`, NOT `PostInc(Neg(a))`,
        // because prefix wraps a full unary-expression (which
        // already folds postfix trailers). This matches C99 §6.5.2
        // / §6.5.3 precedence: postfix binds tighter than prefix.
        let src = "-a++";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = lex_ascii(fid, src);
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_expression(&mut parser).expect("-a++ parses");
        let inner = match e.kind {
            ExprKind::Unary { op: UnOp::Neg, operand } => *operand,
            other => panic!("outer must be Neg, got {other:?}"),
        };
        assert!(matches!(inner.kind, ExprKind::Unary { op: UnOp::PostInc, .. }));
    }

    #[test]
    fn unary_interacts_correctly_with_multiplicative() {
        // `-a * b` parses as `(-a) * b` because the prefix unary is
        // the Pratt LHS leaf — so the `*` infix sees a fully-built
        // `Unary(Neg, a)` on its left, not just `a`.
        let src = "-a * b";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = lex_ascii(fid, src);
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_expression(&mut parser).expect("-a * b parses");
        match e.kind {
            ExprKind::Binary { op: BinOp::Mul, lhs, rhs } => {
                assert!(matches!(lhs.kind, ExprKind::Unary { op: UnOp::Neg, .. }));
                assert!(matches!(rhs.kind, ExprKind::Ident(_)));
            }
            other => panic!("expected top-level `*`, got {other:?}"),
        }
    }

    #[test]
    fn member_access_requires_identifier() {
        // `a.1` — `.` followed by a non-identifier is a syntax
        // error. The diagnostic must point at the offending token
        // and the parser must not spin in the postfix loop.
        let src = "a.1";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = lex_ascii(fid, src);
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        // The outer parse still returns *something* — the recovery
        // path preserves the LHS so downstream layers can make
        // progress — but a diagnostic is emitted.
        let _ = parse_expression(&mut parser);
        let diags = cap.diagnostics();
        assert!(!diags.is_empty(), "member with bad field must diagnose");
        assert!(
            diags[0].message.contains("identifier after `.`"),
            "message mentions `.` after ident, got {:?}",
            diags[0].message,
        );
    }

    #[test]
    fn call_with_missing_rparen_emits_diagnostic() {
        // `f(a` — unclosed call. Recovery: a `Call` node is built
        // with `args = [a]` and an error is emitted; the parser
        // does not enter an infinite loop re-reading the same
        // missing `)`.
        let src = "f(a";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = lex_ascii(fid, src);
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let _ = parse_expression(&mut parser);
        let diags = cap.diagnostics();
        assert!(!diags.is_empty(), "unclosed call must diagnose");
    }

    #[test]
    fn prefix_span_covers_operator_and_operand() {
        // `-a` — span starts at the `-` and ends at the end of `a`.
        let src = "-a";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = lex_ascii(fid, src);
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_expression(&mut parser).expect("-a parses");
        assert_eq!(e.span.lo.0, 0);
        assert_eq!(e.span.hi.0, 2);
    }

    #[test]
    fn postfix_span_covers_base_and_trailer() {
        // `a++` — span from start of `a` to end of `++`.
        let src = "a++";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = lex_ascii(fid, src);
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_expression(&mut parser).expect("a++ parses");
        assert_eq!(e.span.lo.0, 0);
        assert_eq!(e.span.hi.0, 3);
    }

    #[test]
    fn member_access_span_covers_dot_and_field() {
        // `a.b` — span from start of `a` to end of `b`.
        let src = "a.b";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = lex_ascii(fid, src);
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_expression(&mut parser).expect("a.b parses");
        assert!(matches!(e.kind, ExprKind::Member { .. }));
        assert_eq!(e.span.lo.0, 0);
        assert_eq!(e.span.hi.0, 3);
    }
}
