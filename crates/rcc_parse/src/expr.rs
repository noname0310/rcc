//! Expression parsing (C99 §6.5).
//!
//! [`parse_primary`] parses the leaves of the expression grammar.
//! [`parse_expr_bp`] is a Pratt / precedence-climbing loop driven by
//! [`infix_bp`] that folds binary and assignment operators per the C99
//! §6.5 table on top of those leaves. The comma operator (§6.5.17) is
//! wired in as the lowest-binding infix so the two public entry points —
//! [`parse_expression`] (a *full* expression, comma folded) and
//! [`parse_assignment_expression`] (an *assignment-expression*,
//! comma is left as a separator for the caller) — correspond to the
//! two non-terminals C grammar productions spell out.
//!
//! Conditional `?:` is folded here — see [`reduce_conditional`] and
//! the `COND_*_BP` constants below —
//! because its precedence slot (§6.5.15, just above assignment) is
//! inside the Pratt loop proper, even though its ternary shape
//! cannot be described by the plain `(l_bp, r_bp)` infix table.
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
//! conditional     a?b:c              ( 2,  1)*      right  (ternary)
//! assignment      a=b a+=b ...       ( 2,  1)       right
//! comma           a,b                 ( 0,  1)       left
//! ```
//!
//! \*Conditional shares the `(l_bp, r_bp)` numbers with
//! assignment because both sit at the bottom of the table and
//! conditional is handled by a dedicated branch in [`parse_expr_bp`]
//! — the Pratt loop never sees `?` via [`peek_infix`], so there is
//! no ambiguity between the two at the same numeric slot.
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
//! compound literals) are parsed by the postfix / unary layers above
//! the Pratt loop.
//!
//! ## Parenthesised-expression
//!
//! `( expression )` is parsed by [`parse_primary`] with its inner
//! production delegating to [`parse_expr_bp`] at the zero floor, so
//! every operator — including the comma operator (§6.5.17) — may
//! appear inside. That matches the C99 grammar where
//! the parenthesised production is `( expression )`, not
//! `( assignment-expression )`. The error recovery is simple: on a
//! missing inner expression the outer `Paren` arm returns `None`; on
//! a missing `)` it still returns the inner expression unwrapped
//! (not wrapped in `Paren`) and diagnoses the unbalanced paren so
//! the rest of the token stream is not desynchronised.
//!
//! ## Entry-point contract (§6.5.17 vs §6.5.16)
//!
//! C99 distinguishes two non-terminals at the top of the expression
//! grammar:
//!
//! - *expression* (§6.5.17) — folds the comma operator. This is the
//!   thing a statement-expression, the middle of `for (;e;)`, or the
//!   parenthesised-expression production accepts.
//! - *assignment-expression* (§6.5.16) — stops at a top-level `,`.
//!   This is the thing function-call arguments, initialiser
//!   elements, and subscripts inside designators accept, because
//!   the comma in those contexts is a *list separator*, not the
//!   comma operator.
//!
//! The Pratt loop encodes the split with a single constant: the
//! comma's left binding power is [`COMMA_L_BP`] (`0`). Any caller
//! that wants to *exclude* the comma operator from its fold recurses
//! with a minimum binding power of [`COMMA_R_BP`] (`1`, one above
//! comma's left), so the infix loop stops before consuming a `,`.
//! [`parse_expression`] is the `min_bp = 0` entry (folds comma);
//! [`parse_assignment_expression`] is the `min_bp = COMMA_R_BP`
//! entry (leaves comma to the caller).
//!
use rcc_ast::{
    AssignOp, BinOp, CharLiteral as AstCharLiteral, Expr, ExprKind,
    FloatLiteral as AstFloatLiteral, FloatSuffix as AstFloatSuffix, IntLiteral as AstIntLiteral,
    IntSuffix as AstIntSuffix, LiteralEncoding, OffsetofDesignator,
    StringLiteral as AstStringLiteral, UnOp,
};
use rcc_errors::codes;
use rcc_lexer::{Punct, StringEncoding};
use rcc_span::{Span, Symbol};

use crate::decl::parse_type_name;
use crate::init::parse_initializer;
use crate::keywords::Keyword;
use crate::token::{
    CharLiteral, FloatLiteral, FloatSuffix, IntLiteral, IntSuffix, StringLiteral, TokenKind,
};
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
            if is_builtin_type_arg_call(p, sym) {
                return parse_builtin_type_arg_expr(p, sym, span);
            }
            p.bump();
            let id = p.fresh_id();
            Some(Expr { id, kind: ExprKind::Ident(sym), span })
        }
        TokenKind::IntLit(lit) => {
            let lit = ast_int_literal(p, span, lit);
            p.bump();
            let id = p.fresh_id();
            Some(Expr { id, kind: ExprKind::IntLit(lit), span })
        }
        TokenKind::FloatLit(lit) => {
            let lit = ast_float_literal(p, span, lit);
            p.bump();
            let id = p.fresh_id();
            Some(Expr { id, kind: ExprKind::FloatLit(lit), span })
        }
        TokenKind::CharLit(lit) => {
            let lit = ast_char_literal(p, span, lit);
            p.bump();
            let id = p.fresh_id();
            Some(Expr { id, kind: ExprKind::CharLit(lit), span })
        }
        TokenKind::StringLit(lit) => {
            let lit = ast_string_literal(p, span, lit);
            p.bump();
            let id = p.fresh_id();
            Some(Expr { id, kind: ExprKind::StringLit(lit), span })
        }
        TokenKind::Punct(Punct::LParen) => {
            let lparen_span = span;
            p.bump();
            if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::LBrace))) {
                return parse_gnu_statement_expr(p, lparen_span);
            }
            // `( expression )` per §6.5.1 — the inner production is a
            // *full* expression, so the comma operator is allowed
            // here even though it is disallowed at the argument-list
            // layer (§6.5.2.2: each argument is an
            // *assignment-expression*). Recursing with `min_bp = 0`
            // folds comma inside the parentheses.
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

fn parse_gnu_statement_expr(p: &mut Parser<'_>, lparen_span: Span) -> Option<Expr> {
    if !p.session.opts.gnu_statement_expressions {
        p.session
            .handler
            .struct_warn(lparen_span, "GNU statement expression is not part of C99")
            .code(codes::W0013)
            .note("parsing it as an extension so downstream passes can diagnose semantics")
            .emit();
    }

    let block = crate::stmt::parse_block(p)?;
    let rparen_span = match p.peek() {
        Some(t) if matches!(t.kind, TokenKind::Punct(Punct::RParen)) => {
            let span = t.span;
            p.bump();
            span
        }
        Some(t) => {
            p.session
                .handler
                .struct_err(t.span, "expected `)` to close GNU statement expression")
                .label(lparen_span, "statement expression starts here")
                .emit();
            block.span
        }
        None => {
            p.session.handler.struct_err(lparen_span, "unclosed GNU statement expression").emit();
            block.span
        }
    };

    let id = p.fresh_id();
    Some(Expr { id, span: lparen_span.to(rparen_span), kind: ExprKind::StmtExpr(Box::new(block)) })
}

fn is_builtin_type_arg_call(p: &Parser<'_>, sym: Symbol) -> bool {
    let name = p.session.interner.get(sym);
    matches!(name, "__builtin_offsetof" | "__builtin_types_compatible_p")
        && matches!(
            p.tokens.get(p.cursor + 1).map(|t| &t.kind),
            Some(TokenKind::Punct(Punct::LParen))
        )
}

fn parse_builtin_type_arg_expr(p: &mut Parser<'_>, sym: Symbol, name_span: Span) -> Option<Expr> {
    let name = p.session.interner.get(sym).to_owned();
    p.bump(); // builtin identifier

    let lparen_span = match p.peek() {
        Some(t) if matches!(t.kind, TokenKind::Punct(Punct::LParen)) => {
            let span = t.span;
            p.bump();
            span
        }
        _ => {
            let id = p.fresh_id();
            return Some(Expr { id, kind: ExprKind::Ident(sym), span: name_span });
        }
    };

    let (kind, end_span) = match name.as_str() {
        "__builtin_offsetof" => parse_builtin_offsetof_body(p, lparen_span),
        "__builtin_types_compatible_p" => parse_builtin_types_compatible_body(p, lparen_span),
        _ => unreachable!("is_builtin_type_arg_call filters builtin names"),
    };

    let id = p.fresh_id();
    Some(Expr { id, kind, span: name_span.to(end_span) })
}

fn parse_builtin_offsetof_body(p: &mut Parser<'_>, lparen_span: Span) -> (ExprKind, Span) {
    let ty = parse_type_name(p);
    let _ = expect_builtin_comma(p, lparen_span, "__builtin_offsetof");
    let designators = parse_offsetof_designators(p, lparen_span);
    let end_span = expect_builtin_rparen(p, lparen_span, "__builtin_offsetof");
    (ExprKind::BuiltinOffsetof { ty: Box::new(ty), designators }, end_span)
}

fn parse_builtin_types_compatible_body(p: &mut Parser<'_>, lparen_span: Span) -> (ExprKind, Span) {
    let lhs = parse_type_name(p);
    let _ = expect_builtin_comma(p, lparen_span, "__builtin_types_compatible_p");
    let rhs = parse_type_name(p);
    let end_span = expect_builtin_rparen(p, lparen_span, "__builtin_types_compatible_p");
    (ExprKind::BuiltinTypesCompatible { lhs: Box::new(lhs), rhs: Box::new(rhs) }, end_span)
}

fn expect_builtin_comma(p: &mut Parser<'_>, lparen_span: Span, builtin: &str) -> Option<Span> {
    match p.peek() {
        Some(t) if matches!(t.kind, TokenKind::Punct(Punct::Comma)) => {
            let span = t.span;
            p.bump();
            Some(span)
        }
        Some(t) => {
            p.session
                .handler
                .struct_err(t.span, format!("expected `,` in {builtin} argument list"))
                .label(lparen_span, "builtin argument list starts here")
                .emit();
            None
        }
        None => {
            p.session
                .handler
                .struct_err(lparen_span, format!("unclosed {builtin} argument list"))
                .emit();
            None
        }
    }
}

fn expect_builtin_rparen(p: &mut Parser<'_>, lparen_span: Span, builtin: &str) -> Span {
    match p.peek() {
        Some(t) if matches!(t.kind, TokenKind::Punct(Punct::RParen)) => {
            let span = t.span;
            p.bump();
            span
        }
        Some(t) => {
            let span = t.span;
            p.session
                .handler
                .struct_err(span, format!("expected `)` to close {builtin} argument list"))
                .label(lparen_span, "unmatched `(` here")
                .emit();
            span
        }
        None => {
            p.session
                .handler
                .struct_err(lparen_span, format!("unclosed {builtin} argument list"))
                .emit();
            lparen_span
        }
    }
}

fn parse_offsetof_designators(p: &mut Parser<'_>, lparen_span: Span) -> Vec<OffsetofDesignator> {
    let mut designators = Vec::new();

    match p.peek() {
        Some(t) if matches!(t.kind, TokenKind::Punct(Punct::Dot)) => {
            let dot_span = t.span;
            p.bump();
            if !push_offsetof_field(p, dot_span, "`.`", &mut designators) {
                return designators;
            }
        }
        Some(t) if matches!(t.kind, TokenKind::Ident(_)) => {
            let TokenKind::Ident(field) = t.kind else { unreachable!() };
            p.bump();
            designators.push(OffsetofDesignator::Field(field));
        }
        Some(t) => {
            p.session
                .handler
                .struct_err(t.span, "expected member designator in __builtin_offsetof")
                .label(lparen_span, "offsetof argument list starts here")
                .emit();
            return designators;
        }
        None => {
            p.session
                .handler
                .struct_err(lparen_span, "unclosed __builtin_offsetof argument list")
                .emit();
            return designators;
        }
    }

    loop {
        match p.peek() {
            Some(t) if matches!(t.kind, TokenKind::Punct(Punct::Dot)) => {
                let dot_span = t.span;
                p.bump();
                if !push_offsetof_field(p, dot_span, "`.`", &mut designators) {
                    break;
                }
            }
            Some(t) if matches!(t.kind, TokenKind::Punct(Punct::LBracket)) => {
                let lbracket_span = t.span;
                p.bump();
                let Some(index) = parse_expression(p) else {
                    break;
                };
                let _ = expect_offsetof_rbracket(p, lbracket_span);
                designators.push(OffsetofDesignator::Index(Box::new(index)));
            }
            _ => break,
        }
    }

    designators
}

fn push_offsetof_field(
    p: &mut Parser<'_>,
    op_span: Span,
    op_spelling: &str,
    designators: &mut Vec<OffsetofDesignator>,
) -> bool {
    match p.peek() {
        Some(t) => {
            if let TokenKind::Ident(field) = t.kind {
                p.bump();
                designators.push(OffsetofDesignator::Field(field));
                true
            } else {
                p.session
                    .handler
                    .struct_err(t.span, format!("expected identifier after {op_spelling}"))
                    .label(op_span, "offsetof member access here")
                    .emit();
                false
            }
        }
        None => {
            p.session
                .handler
                .struct_err(op_span, format!("expected identifier after {op_spelling}"))
                .emit();
            false
        }
    }
}

fn expect_offsetof_rbracket(p: &mut Parser<'_>, lbracket_span: Span) -> Option<Span> {
    match p.peek() {
        Some(t) if matches!(t.kind, TokenKind::Punct(Punct::RBracket)) => {
            let span = t.span;
            p.bump();
            Some(span)
        }
        Some(t) => {
            p.session
                .handler
                .struct_err(t.span, "expected `]` to close __builtin_offsetof subscript")
                .label(lbracket_span, "unmatched `[` here")
                .emit();
            None
        }
        None => {
            p.session
                .handler
                .struct_err(lbracket_span, "unclosed __builtin_offsetof subscript")
                .emit();
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
/// whose disambiguation needs typedef-name lookahead. Both forms are
/// parsed here alongside the cast expression
/// (§6.5.4); see [`parse_sizeof`] and [`parse_cast`] for the
/// disambiguation code. Compound literals `( type-name ) { … }`
/// share the same `(` lookahead; [`parse_cast`] disambiguates by
/// peeking for `{` after the closing `)`.
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

    // `sizeof`-expression (§6.5.3.4). Two shapes: `sizeof ( type-name )`
    // and `sizeof unary-expression`; both handled by the helper.
    if matches!(tok.kind, TokenKind::Keyword(Keyword::Sizeof)) {
        return parse_sizeof(p);
    }

    // Cast-expression (§6.5.4): `( type-name ) cast-expression`.
    // We only take this branch when the one-token lookahead past `(`
    // unambiguously starts a type-name; otherwise we fall through to
    // `parse_postfix` → `parse_primary`, which handles the ordinary
    // parenthesised-expression shape and the three-way ambiguity
    // with `( ident )` described on [`starts_type_name_after_lparen`].
    if matches!(tok.kind, TokenKind::Punct(Punct::LParen)) && starts_type_name_after_lparen(p) {
        return parse_cast(p);
    }

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

/// Parse a C99 *cast-expression* (§6.5.4) or *compound literal*
/// (§6.5.2.5). Both begin with `( type-name )`; the token after
/// the closing `)` disambiguates:
///
/// - `{` → compound literal: call [`parse_initializer`] for the
///   braced body, then feed the result through [`parse_postfix_tail`]
///   so trailing `.field` / `->field` / `[idx]` / `++` / `--` are
///   consumed (compound literal is a postfix-expression per §6.5.2).
/// - anything else → cast: recurse into [`parse_prefix_unary`] for
///   the operand (`(int)-x`, `(int)(long)y`, `(int)x++`, …).
fn parse_cast(p: &mut Parser<'_>) -> Option<Expr> {
    let lparen_span = p.cur_span();
    p.bump(); // `(`
    let ty = parse_type_name(p);
    expect_rparen_after_type(p, lparen_span, "cast / compound literal");

    if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::LBrace))) {
        let init = parse_initializer(p)?;
        let end = p.tokens.get(p.cursor.wrapping_sub(1)).map(|t| t.span).unwrap_or(lparen_span);
        let span = lparen_span.to(end);
        let id = p.fresh_id();
        let lit = Expr { id, kind: ExprKind::CompoundLiteral { ty, init: Box::new(init) }, span };
        return Some(parse_postfix_tail(p, lit));
    }

    let operand = parse_prefix_unary(p)?;
    let span = lparen_span.to(operand.span);
    let id = p.fresh_id();
    Some(Expr { id, kind: ExprKind::Cast { ty, expr: Box::new(operand) }, span })
}

/// Parse a C99 §6.5.3.4 *sizeof-expression*. Two shapes:
///
/// - `sizeof ( type-name )` — produces [`ExprKind::SizeofType`]
/// - `sizeof unary-expression` — produces [`ExprKind::SizeofExpr`],
///   wrapping whatever [`parse_prefix_unary`] returns (which itself
///   folds further casts, sizeofs, and prefix operators).
///
/// Disambiguation mirrors the cast path: a following `(` starts a
/// type-name only when the one-token lookahead past it is a
/// type-specifier / type-qualifier keyword, or an identifier that
/// the scope stack currently classifies as a typedef-name (§6.7.2p2
/// footnote). Everything else — including `sizeof (x)` with an
/// ordinary `x` — is the unary-expression form; the outer
/// [`parse_primary`] will later unwrap the inner paren-expression.
fn parse_sizeof(p: &mut Parser<'_>) -> Option<Expr> {
    let kw_span = p.cur_span();
    p.bump(); // `sizeof`

    // `sizeof ( type-name )` — the `(` must immediately follow
    // `sizeof` AND the token past it must start a type-name.
    if let Some(tok) = p.peek() {
        if matches!(tok.kind, TokenKind::Punct(Punct::LParen)) && starts_type_name_after_lparen(p) {
            let lparen_span = tok.span;
            p.bump(); // `(`
            let ty = parse_type_name(p);
            let end_span = expect_rparen_after_type(p, lparen_span, "sizeof");
            let span = kw_span.to(end_span.unwrap_or(kw_span));
            let id = p.fresh_id();
            return Some(Expr { id, kind: ExprKind::SizeofType(ty), span });
        }
    }

    // `sizeof unary-expression` — the operand may itself be
    // parenthesised as `sizeof(expr)`, which falls through the
    // non-type branch above and reaches here; the `(` then parses
    // as a primary paren-expression inside the recursive call.
    let operand = parse_prefix_unary(p)?;
    let span = kw_span.to(operand.span);
    let id = p.fresh_id();
    Some(Expr { id, kind: ExprKind::SizeofExpr(Box::new(operand)), span })
}

/// One-token lookahead: does the token immediately after the current
/// `(` start a C99 *type-name* (§6.7.6)?
///
/// A type-name begins with a *specifier-qualifier-list*, which in
/// turn may start with any of:
///
/// - a *type-specifier* keyword — `void`, `char`, `short`, `int`,
///   `long`, `float`, `double`, `signed`, `unsigned`, `_Bool`,
///   `_Complex`, `_Imaginary`, `struct`, `union`, `enum`.
/// - a *type-qualifier* keyword — `const`, `volatile`, `restrict`.
/// - an identifier that names a typedef — detected via
///   [`crate::scope::ScopeStack::is_typedef`].
///
/// The caller must have already confirmed the current token is `(`;
/// this helper looks at `p.cursor + 1`. The check is only
/// side-effect-free peeking, so the caller can still fall through
/// to the paren-expression path if it returns `false`.
fn starts_type_name_after_lparen(p: &Parser<'_>) -> bool {
    match p.tokens.get(p.cursor + 1).map(|t| &t.kind) {
        Some(TokenKind::Keyword(kw)) => is_type_name_start_kw(*kw),
        Some(TokenKind::Ident(sym)) => p.scopes.is_typedef(*sym),
        _ => false,
    }
}

/// Keyword predicate for [`starts_type_name_after_lparen`]. Covers
/// every keyword that can sit at the front of a
/// *specifier-qualifier-list* (§6.7.2 type specifiers + §6.7.3 type
/// qualifiers). Non-type keywords — control-flow, storage class,
/// function specifier — return `false` so we fall through to the
/// paren-expression path for shapes like `(sizeof x)` or `(inline
/// ...)` (the latter is ill-formed but that's for later passes).
fn is_type_name_start_kw(kw: Keyword) -> bool {
    matches!(
        kw,
        Keyword::Void
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
            | Keyword::Struct
            | Keyword::Union
            | Keyword::Enum
            | Keyword::Const
            | Keyword::Volatile
            | Keyword::Restrict
    )
}

/// Consume the closing `)` after a type-name in a cast or sizeof
/// construct. Returns the `)`'s span on success so the caller can
/// stitch a tight overall span; on a missing `)` we emit a
/// diagnostic that points at both the current cursor and the `(`
/// that opened the construct, and return `None` — the caller then
/// falls back to the keyword span so downstream nodes still have
/// *some* span to hang diagnostics on.
fn expect_rparen_after_type(p: &mut Parser<'_>, lparen_span: Span, ctx: &str) -> Option<Span> {
    match p.peek() {
        Some(t) if matches!(t.kind, TokenKind::Punct(Punct::RParen)) => {
            let s = t.span;
            p.bump();
            Some(s)
        }
        _ => {
            p.session
                .handler
                .struct_err(p.cur_span(), format!("expected `)` after type name in {ctx}"))
                .label(lparen_span, "type name begins here")
                .emit();
            None
        }
    }
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
/// Returns `None` only when [`parse_primary`] cannot produce an
/// initial expression; mid-chain syntax errors (e.g. `a.` with no
/// field, `f(` with no closing paren) are diagnosed and the
/// partially-built expression is returned so that higher layers can
/// keep parsing.
pub fn parse_postfix(p: &mut Parser<'_>) -> Option<Expr> {
    let lhs = parse_primary(p)?;
    Some(parse_postfix_tail(p, lhs))
}

/// Consume zero or more postfix trailers (`[…]`, `.`, `->`, `++`,
/// `--`, `(…)`) starting from a pre-built `lhs`. Factored out of
/// [`parse_postfix`] so that [`parse_cast`] can feed a compound-
/// literal expression into the same postfix loop without going
/// through `parse_primary`.
fn parse_postfix_tail(p: &mut Parser<'_>, mut lhs: Expr) -> Expr {
    while let Some((punct, op_span)) = p.peek().and_then(|t| match t.kind {
        TokenKind::Punct(pu) => Some((pu, t.span)),
        _ => None,
    }) {
        match punct {
            // `a[b]` — §6.5.2.1 array subscript. The bracketed
            // production is a *full expression*, so comma is allowed
            // here (`a[b, c]` is legal, if unusual, C). Recursing at
            // `min_bp = 0` folds comma inside the brackets, exactly
            // like the parenthesised-expression arm does.
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
            // Argument parsing runs through
            // [`parse_assignment_expression`], which recurses with
            // `min_bp = COMMA_R_BP` so a top-level `,` stays a list
            // separator instead of folding into an
            // [`ExprKind::Comma`]; a nested `(a, b)` still builds a
            // comma-expression because the parenthesised-expression
            // arm re-enters at `min_bp = 0`.
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
    lhs
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
/// Each argument is an *assignment-expression* per §6.5.2.2 ¶1, so
/// we delegate to [`parse_assignment_expression`] — which runs the
/// Pratt loop with a minimum binding power of [`COMMA_R_BP`] — and
/// leave the top-level `,` visible to this function's own
/// separator-dispatch below. A nested `(a, b)` still reaches the
/// comma operator because the parenthesised-expression arm of
/// [`parse_primary`] re-enters the loop at `min_bp = 0`.
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
        let Some(arg) = parse_assignment_expression(p) else {
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

/// Top-level C99 *expression* entry point (§6.5.17).
///
/// Drives a Pratt / precedence-climbing loop starting at the lowest
/// binding power (`min_bp = 0`), so every operator in the table
/// documented at the module level is accepted — including the
/// comma operator, which folds left-associatively into a chain of
/// [`ExprKind::Comma`] nodes.
///
/// This is the entry point to use whenever the C grammar spells the
/// context as *expression*: the middle clause of `for (; e ; )`, the
/// body of a parenthesised expression `( expression )`, the
/// expression-statement (§6.8.3), and the subscript slot
/// `a [ expression ]` (§6.5.2.1). Every other context where commas
/// act as list separators — function-call arguments (§6.5.2.2),
/// initialiser elements (§6.7.8), etc. — must call
/// [`parse_assignment_expression`] instead so that the `,` remains
/// visible to the caller's separator logic.
///
/// Returns `None` when no primary expression is available at the
/// cursor position; in that case a diagnostic has already been
/// emitted by [`parse_primary`] and the cursor is left where the
/// error happened so the caller can decide how to recover.
pub fn parse_expression(p: &mut Parser<'_>) -> Option<Expr> {
    parse_expr_bp(p, 0)
}

/// Parse a C99 *assignment-expression* (§6.5.16).
///
/// Runs the same Pratt loop as [`parse_expression`] but with a
/// minimum binding power of [`COMMA_R_BP`], which is strictly
/// greater than the comma operator's left binding power
/// ([`COMMA_L_BP`]). That makes the loop stop before consuming a
/// top-level `,`, matching the §6.5.16 grammar — the caller (e.g.
/// [`parse_call_args`]) then treats the comma as its own separator.
///
/// A nested `( a , b )` still builds a comma-expression, because
/// the parenthesised-expression arm in [`parse_primary`] re-enters
/// the loop at `min_bp = 0` — the "min_bp" restriction is scoped to
/// the current Pratt call, not the whole subtree.
pub fn parse_assignment_expression(p: &mut Parser<'_>) -> Option<Expr> {
    parse_expr_bp(p, COMMA_R_BP)
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
    // The Pratt leaf is `parse_prefix_unary`, which runs its own
    // left-to-right prefix-operator chain before
    // falling back to `parse_postfix`, which in turn consumes the
    // primary and any trailing `[...]`, `.`, `->`, `++`, `--`, or
    // `( args )` postfix trailers. This keeps all three layers —
    // prefix unary, postfix trailers, and binary / assignment infix
    // — in their C99 §6.5 precedence order without any bp numbering
    // for the unary levels themselves (their relative strength is
    // encoded by *where* in the call graph they live).
    let mut lhs = parse_prefix_unary(p)?;
    loop {
        // `?` (C99 §6.5.15) is not an infix operator in the sense
        // `peek_infix` knows — its RHS is a pair `then : else` — so
        // we pattern-match it here, before the binary/assignment
        // path, when the Pratt floor allows it.
        if let Some(q_span) = peek_question(p) {
            if COND_L_BP < min_bp {
                break;
            }
            match reduce_conditional(p, lhs, q_span) {
                Ok(new_lhs) => {
                    lhs = new_lhs;
                    continue;
                }
                Err(recovered_lhs) => {
                    // Recovery path: the `?:` reducer already emitted
                    // a diagnostic (missing `:`, missing then/else
                    // operand). Returning the partially-built LHS
                    // keeps downstream phases from seeing a
                    // synthesised node with a dummy span.
                    return Some(*recovered_lhs);
                }
            }
        }
        let Some(op) = peek_infix(p) else { break };
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
            InfixOp::Comma => {
                Expr { id, kind: ExprKind::Comma { lhs: Box::new(lhs), rhs: Box::new(rhs) }, span }
            }
        };
    }
    Some(lhs)
}

/// Left binding power of the comma operator `,` (§6.5.17).
///
/// Comma sits at the very bottom of the C precedence table — every
/// other operator binds tighter. The left-binding-power is `0` so it
/// is *only* folded when the Pratt loop is entered at `min_bp = 0`
/// (i.e. via [`parse_expression`]); callers that want to keep commas
/// as list separators enter at [`COMMA_R_BP`] via
/// [`parse_assignment_expression`].
const COMMA_L_BP: u8 = 0;

/// Right binding power of the comma operator `,`.
///
/// Setting the pair to `(0, 1)` encodes left-associativity in the
/// Matklad `(l_bp, r_bp)` convention: after we consume one `,`, we
/// recurse with `min_bp = 1`, which is strictly above
/// [`COMMA_L_BP`], so the RHS cannot itself grow another top-level
/// `,` — forcing the expected `(a , b) , c` shape for `a, b, c`. It
/// also doubles as the "assignment-expression floor": recursing at
/// `min_bp = 1` lets every operator above comma (including
/// assignment, whose left binding power is `2`) fold inside the
/// RHS, exactly mirroring the §6.5.16 grammar.
const COMMA_R_BP: u8 = 1;

/// Left binding power of the conditional operator `?:` (§6.5.15).
///
/// The C99 precedence table places conditional *just above*
/// assignment and *just below* logical-OR. In the Matklad encoding
/// that means:
///
/// - `COND_L_BP (2) > assignment.r_bp (1)` so that `a = b ? c : d`
///   pulls the whole `b ? c : d` into the assignment RHS.
/// - `COND_L_BP (2) < LogOr.r_bp (4)` so that `a || b ? c : d`
///   folds `a || b` first — the cond operator never fires inside
///   the LogOr recursion — matching the C99 grammar where the
///   first operand of `?:` is a *logical-OR-expression*.
const COND_L_BP: u8 = 2;

/// Right binding power for the *else* operand. Must be strictly
/// less than [`COND_L_BP`] so that `a ? b : c ? d : e` re-enters
/// the conditional branch on the right and yields the
/// §6.5.15-mandated `a ? b : (c ? d : e)` shape. Setting it to `1`
/// also lets an assignment-expression appear in the else slot —
/// the permissive reading shared by gcc, clang, and chibicc —
/// without disturbing the right-associativity of `?:`. A stricter
/// "C99 conditional-expression only" reading would bump this to
/// `3`, but every real-world C compiler accepts the permissive
/// form and the lvalue rule is enforced downstream in typeck.
const COND_R_BP: u8 = 1;

/// Peek the current token and return its span if it is `?`,
/// without advancing the cursor. Returns `None` for every other
/// token — including end-of-input — so the Pratt loop falls
/// through to its regular infix handling.
fn peek_question(p: &Parser<'_>) -> Option<rcc_span::Span> {
    let t = p.peek()?;
    if matches!(t.kind, TokenKind::Punct(Punct::Question)) {
        Some(t.span)
    } else {
        None
    }
}

/// Reduce a conditional expression given the already-parsed
/// first operand (`cond`) and the span of the `?` we are about
/// to consume. On success, returns `Ok(Cond { .. })`; on any
/// recovery path — missing `then`, missing `:`, missing `else`
/// — emits a diagnostic and returns `Err(Box<cond>)` so the
/// caller can hand the partially-built LHS back unwrapped.
/// Returning `cond` on failure (rather than `None`) is how we
/// thread the original LHS ownership back out past the consumed
/// `?` without cloning a whole sub-tree; boxing the `Err`
/// variant keeps the `Result` itself pointer-sized per the
/// `clippy::result_large_err` guidance.
///
/// The shape `cond ? then : else` is parsed as:
///
/// - `then`: a full expression via [`parse_expression`]. Per
///   §6.5.15, the second operand of `?:` is an *expression*
///   (comma operator included), not merely a
///   conditional-expression.
/// - `else`: a recursive [`parse_expr_bp`] call at
///   [`COND_R_BP`], which both preserves §6.5.15 right-
///   associativity and lets an assignment-expression fill the
///   slot (matching real compilers).
///
/// Spans: the produced `Cond` node spans `cond.lo .. else.hi`
/// — the widest range that still comes from real source text,
/// without synthesising a span for the operator punctuation.
fn reduce_conditional(
    p: &mut Parser<'_>,
    cond: Expr,
    q_span: rcc_span::Span,
) -> Result<Expr, Box<Expr>> {
    // Consume the `?`.
    p.bump();
    // `then` is a full expression — no Pratt floor, so any
    // operator weaker than `:` can appear inside. Failure leaves
    // the cursor on the offending token with a diagnostic
    // already emitted by `parse_primary`.
    let Some(then_expr) = parse_expression(p) else {
        return Err(Box::new(cond));
    };
    // Expect `:`.
    match p.peek() {
        Some(t) if matches!(t.kind, TokenKind::Punct(Punct::Colon)) => {
            p.bump();
        }
        _ => {
            let at = p.cur_span();
            p.session
                .handler
                .struct_err(at, "expected `:` in conditional expression")
                .label(q_span, "`?` here")
                .emit();
            return Err(Box::new(cond));
        }
    }
    // `else` binds right-associatively. `COND_R_BP < COND_L_BP`
    // allows another `?:` (or a weaker-binding assignment) to
    // nest on the right; any operator stronger than cond is
    // already handled by the Pratt recursion.
    let Some(else_expr) = parse_expr_bp(p, COND_R_BP) else {
        return Err(Box::new(cond));
    };
    let span = cond.span.to(else_expr.span);
    let id = p.fresh_id();
    Ok(Expr {
        id,
        kind: ExprKind::Cond {
            cond: Box::new(cond),
            then_expr: Box::new(then_expr),
            else_expr: Box::new(else_expr),
        },
        span,
    })
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
    /// The C99 §6.5.17 comma operator. It does not map to `BinOp`
    /// because semantically it is neither arithmetic nor bitwise —
    /// it sequences two evaluations and yields the value of the
    /// right-hand operand. Keeping it as its own variant means the
    /// fold site in [`parse_expr_bp`] dispatches to
    /// [`ExprKind::Comma`] directly instead of stuffing a fake
    /// `BinOp::Comma` through the general binary path (which would
    /// muddy the arithmetic-oriented `BinOp` enum and its folding
    /// rules in typeck / HIR lowering).
    Comma,
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
        // Comma operator (§6.5.17). Reported here so [`parse_expr_bp`]
        // folds it through the same machinery as every other infix;
        // the `min_bp` floor at call sites that want comma as a
        // separator (e.g. [`parse_assignment_expression`]) naturally
        // bails out before consuming it.
        Punct::Comma => InfixOp::Comma,
        // Everything else — including `?`, `:`, and all brackets /
        // delimiters — is NOT an infix operator at this layer.
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
        // Level 0: comma — lowest in §6.5, left-associative.
        InfixOp::Comma => (COMMA_L_BP, COMMA_R_BP),
        // Level 1: assignment — right-associative, just above comma.
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

fn ast_int_literal(p: &mut Parser<'_>, span: Span, lit: IntLiteral) -> AstIntLiteral {
    AstIntLiteral {
        text: intern_span_text(p, span),
        value: lit.value,
        suffix: match lit.suffix {
            IntSuffix::None => AstIntSuffix::None,
            IntSuffix::U => AstIntSuffix::U,
            IntSuffix::L => AstIntSuffix::L,
            IntSuffix::UL => AstIntSuffix::UL,
            IntSuffix::LL => AstIntSuffix::LL,
            IntSuffix::ULL => AstIntSuffix::ULL,
        },
    }
}

fn ast_float_literal(p: &mut Parser<'_>, span: Span, lit: FloatLiteral) -> AstFloatLiteral {
    AstFloatLiteral {
        text: intern_span_text(p, span),
        value: lit.value,
        suffix: match lit.suffix {
            FloatSuffix::None => AstFloatSuffix::None,
            FloatSuffix::F => AstFloatSuffix::F,
            FloatSuffix::L => AstFloatSuffix::L,
        },
    }
}

fn ast_char_literal(p: &mut Parser<'_>, span: Span, lit: CharLiteral) -> AstCharLiteral {
    AstCharLiteral {
        text: intern_span_text(p, span),
        value: lit.value,
        encoding: ast_literal_encoding(lit.encoding),
    }
}

fn ast_string_literal(p: &mut Parser<'_>, span: Span, lit: StringLiteral) -> AstStringLiteral {
    AstStringLiteral {
        text: intern_span_text(p, span),
        bytes: lit.bytes,
        encoding: ast_literal_encoding(lit.encoding),
    }
}

fn ast_literal_encoding(enc: StringEncoding) -> LiteralEncoding {
    match enc {
        StringEncoding::None => LiteralEncoding::None,
        StringEncoding::Utf8 => LiteralEncoding::Utf8,
        StringEncoding::Utf16 => LiteralEncoding::Utf16,
        StringEncoding::Utf32 => LiteralEncoding::Utf32,
        StringEncoding::Wide => LiteralEncoding::Wide,
    }
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
        // `42` → ExprKind::IntLit with raw spelling + decoded payload.
        let src = "42";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = [pp(PpTokenKind::PpNumber(PpNumberKind::Integer), fid, 0, 2)];
        let tokens = convert(&mut sess, &pps);
        match &tokens[0].kind {
            TokenKind::IntLit(lit) => assert_eq!(lit.value, 42),
            other => panic!("expected IntLit, got {other:?}"),
        }
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_primary(&mut parser).expect("42 parses");
        match e.kind {
            ExprKind::IntLit(lit) => {
                assert_eq!(parser.session.interner.get(lit.text), "42");
                assert_eq!(lit.value, 42);
                assert_eq!(lit.suffix, AstIntSuffix::None);
            }
            other => panic!("expected IntLit, got {other:?}"),
        }
        assert_eq!(e.span.lo.0, 0);
        assert_eq!(e.span.hi.0, 2);
        // Cursor advanced past the consumed token.
        assert_eq!(parser.cursor, 1);
    }

    #[test]
    fn integer_literal_suffix_reaches_ast_payload() {
        let src = "42UL";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = [pp(PpTokenKind::PpNumber(PpNumberKind::Integer), fid, 0, 4)];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_primary(&mut parser).expect("42UL parses");
        match e.kind {
            ExprKind::IntLit(lit) => {
                assert_eq!(parser.session.interner.get(lit.text), "42UL");
                assert_eq!(lit.value, 42);
                assert_eq!(lit.suffix, AstIntSuffix::UL);
            }
            other => panic!("expected IntLit, got {other:?}"),
        }
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
            ExprKind::StringLit(lit) => {
                assert_eq!(parser.session.interner.get(lit.text), "\"hi\"");
                assert_eq!(lit.bytes, b"hi");
                assert_eq!(lit.encoding, LiteralEncoding::None);
            }
            other => panic!("expected StringLit, got {other:?}"),
        }
    }

    #[test]
    fn adjacent_string_literals_reach_ast_as_concatenated_payload() {
        let src = "\"a\" \"b\"";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = [
            pp(PpTokenKind::StringLit { enc: StringEncoding::None }, fid, 0, 3),
            pp(PpTokenKind::Whitespace, fid, 3, 4),
            pp(PpTokenKind::StringLit { enc: StringEncoding::None }, fid, 4, 7),
        ];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_primary(&mut parser).expect("adjacent strings parse");
        match e.kind {
            ExprKind::StringLit(lit) => {
                assert_eq!(parser.session.interner.get(lit.text), "\"a\" \"b\"");
                assert_eq!(lit.bytes, b"ab");
                assert_eq!(lit.encoding, LiteralEncoding::None);
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
            ExprKind::CharLit(lit) => {
                assert_eq!(parser.session.interner.get(lit.text), "'a'");
                assert_eq!(lit.value, u32::from(b'a'));
                assert_eq!(lit.encoding, LiteralEncoding::None);
            }
            other => panic!("expected CharLit, got {other:?}"),
        }
    }

    #[test]
    fn char_literal_encoding_reaches_ast_payload() {
        let src = "L'a'";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = [pp(PpTokenKind::CharConst { enc: StringEncoding::Wide }, fid, 0, 4)];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_primary(&mut parser).expect("wide char parses");
        match e.kind {
            ExprKind::CharLit(lit) => {
                assert_eq!(parser.session.interner.get(lit.text), "L'a'");
                assert_eq!(lit.value, u32::from(b'a'));
                assert_eq!(lit.encoding, LiteralEncoding::Wide);
            }
            other => panic!("expected CharLit, got {other:?}"),
        }
    }

    #[test]
    fn float_literal_parses_to_floatlit() {
        // `2.5` → ExprKind::FloatLit.
        let src = "2.5";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = [pp(PpTokenKind::PpNumber(PpNumberKind::Float), fid, 0, 3)];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_primary(&mut parser).expect("2.5 parses");
        match e.kind {
            ExprKind::FloatLit(lit) => {
                assert_eq!(parser.session.interner.get(lit.text), "2.5");
                assert_eq!(lit.value, 2.5);
                assert_eq!(lit.suffix, AstFloatSuffix::None);
            }
            other => panic!("expected FloatLit, got {other:?}"),
        }
    }

    #[test]
    fn float_literal_suffix_reaches_ast_payload() {
        let src = "1.5f";
        let (mut sess, fid, _cap) = mk_session(src);
        let pps = [pp(PpTokenKind::PpNumber(PpNumberKind::Float), fid, 0, 4)];
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_primary(&mut parser).expect("1.5f parses");
        match e.kind {
            ExprKind::FloatLit(lit) => {
                assert_eq!(parser.session.interner.get(lit.text), "1.5f");
                assert_eq!(lit.value, 1.5);
                assert_eq!(lit.suffix, AstFloatSuffix::F);
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
                ExprKind::IntLit(lit) => {
                    assert_eq!(parser.session.interner.get(lit.text), "42");
                    assert_eq!(lit.value, 42);
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
        assert!(matches!(l3.kind, ExprKind::IntLit(_)));
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
        assert!(matches!(e.kind, ExprKind::IntLit(_)));
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
                    b'?' => Punct::Question,
                    b':' => Punct::Colon,
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
        assert!(matches!(cur.kind, ExprKind::IntLit(_)));
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
        assert!(matches!(idx_index.kind, ExprKind::IntLit(_)), "index must be `0`");
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
        // decision so comma-expression support does not silently
        // regress it.
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

    // ── 05-11 conditional `?:` (§6.5.15) ────────────────────────────

    #[test]
    fn conditional_basic_builds_cond_node() {
        // `a ? b : c` → Cond { cond = a, then = b, else = c }.
        let src = "a ? b : c";
        let (e, cap) = parse_expr_str(src);
        assert!(cap.diagnostics().is_empty(), "valid `?:` must be diag-free");
        match e.kind {
            ExprKind::Cond { cond, then_expr, else_expr } => {
                assert!(matches!(cond.kind, ExprKind::Ident(_)), "cond must be `a`");
                assert!(matches!(then_expr.kind, ExprKind::Ident(_)), "then must be `b`");
                assert!(matches!(else_expr.kind, ExprKind::Ident(_)), "else must be `c`");
            }
            other => panic!("expected Cond, got {other:?}"),
        }
        // Span must cover the whole run.
        assert_eq!(e.span.lo.0, 0);
        assert_eq!(e.span.hi.0, src.len() as u32);
    }

    #[test]
    fn conditional_is_right_associative_in_else_arm() {
        // Acceptance (§6.5.15): `a ? b : c ? d : e` parses as
        // `a ? b : (c ? d : e)` — the else-operand is itself a
        // conditional-expression, so the inner `?:` nests on the right.
        let (e, _cap) = parse_expr_str("a ? b : c ? d : e");
        match e.kind {
            ExprKind::Cond { cond, then_expr, else_expr } => {
                assert!(matches!(cond.kind, ExprKind::Ident(_)), "outer cond must be `a`");
                assert!(matches!(then_expr.kind, ExprKind::Ident(_)), "outer then must be `b`");
                match else_expr.kind {
                    ExprKind::Cond {
                        cond: inner_cond,
                        then_expr: inner_then,
                        else_expr: inner_else,
                    } => {
                        assert!(matches!(inner_cond.kind, ExprKind::Ident(_)));
                        assert!(matches!(inner_then.kind, ExprKind::Ident(_)));
                        assert!(matches!(inner_else.kind, ExprKind::Ident(_)));
                    }
                    other => panic!("outer else must be inner Cond, got {other:?}"),
                }
            }
            other => panic!("expected top-level Cond, got {other:?}"),
        }
    }

    #[test]
    fn conditional_nests_inside_then_arm() {
        // `a ? b ? c : d : e` — the then-operand is parsed as a *full*
        // expression (§6.5.15: second operand), so another `?:` is
        // perfectly legal between the outer `?` and `:`. Expected
        // tree: `a ? (b ? c : d) : e`.
        let (e, _cap) = parse_expr_str("a ? b ? c : d : e");
        match e.kind {
            ExprKind::Cond { cond, then_expr, else_expr } => {
                assert!(matches!(cond.kind, ExprKind::Ident(_)), "outer cond must be `a`");
                assert!(matches!(else_expr.kind, ExprKind::Ident(_)), "outer else must be `e`");
                match then_expr.kind {
                    ExprKind::Cond {
                        cond: inner_cond,
                        then_expr: inner_then,
                        else_expr: inner_else,
                    } => {
                        assert!(matches!(inner_cond.kind, ExprKind::Ident(_)));
                        assert!(matches!(inner_then.kind, ExprKind::Ident(_)));
                        assert!(matches!(inner_else.kind, ExprKind::Ident(_)));
                    }
                    other => panic!("outer then must be inner Cond, got {other:?}"),
                }
            }
            other => panic!("expected top-level Cond, got {other:?}"),
        }
    }

    #[test]
    fn conditional_folds_inside_assignment_rhs() {
        // `a = b ? c : d` → `a = (b ? c : d)`. Conditional sits just
        // above assignment (§6.5.15 vs §6.5.16), so when the Pratt
        // loop recurses into the assignment RHS it picks `?` up.
        let (e, _cap) = parse_expr_str("a = b ? c : d");
        match e.kind {
            ExprKind::Assign { op: AssignOp::Eq, lhs, rhs } => {
                assert!(matches!(lhs.kind, ExprKind::Ident(_)), "lhs must be `a`");
                match rhs.kind {
                    ExprKind::Cond { cond, then_expr, else_expr } => {
                        assert!(matches!(cond.kind, ExprKind::Ident(_)));
                        assert!(matches!(then_expr.kind, ExprKind::Ident(_)));
                        assert!(matches!(else_expr.kind, ExprKind::Ident(_)));
                    }
                    other => panic!("rhs must be Cond, got {other:?}"),
                }
            }
            other => panic!("expected top-level assignment, got {other:?}"),
        }
    }

    #[test]
    fn conditional_lhs_includes_full_logical_or() {
        // `a || b ? c : d` → `(a || b) ? c : d`. The first operand of
        // `?:` is a *logical-OR-expression* per §6.5.15, so `||` binds
        // *tighter* than `?:` and the whole disjunction folds into the
        // cond slot.
        let (e, _cap) = parse_expr_str("a || b ? c : d");
        match e.kind {
            ExprKind::Cond { cond, then_expr, else_expr } => {
                match cond.kind {
                    ExprKind::Binary { op: BinOp::LogOr, .. } => {}
                    other => panic!("cond must be `a || b`, got {other:?}"),
                }
                assert!(matches!(then_expr.kind, ExprKind::Ident(_)), "then must be `c`");
                assert!(matches!(else_expr.kind, ExprKind::Ident(_)), "else must be `d`");
            }
            other => panic!("expected top-level Cond, got {other:?}"),
        }
    }

    #[test]
    fn conditional_else_branch_absorbs_assignment() {
        // Pragmatic extension matched by gcc/clang: the else-operand
        // allows an assignment-expression. `a ? b : c = d` parses as
        // `a ? b : (c = d)`. Semantically the `=` needs an lvalue, but
        // that is checked by typeck — the parser is permissive. This
        // pins the behaviour down so comma-expression support does
        // not accidentally regress it.
        let (e, _cap) = parse_expr_str("a ? b : c = d");
        match e.kind {
            ExprKind::Cond { cond, then_expr, else_expr } => {
                assert!(matches!(cond.kind, ExprKind::Ident(_)), "cond must be `a`");
                assert!(matches!(then_expr.kind, ExprKind::Ident(_)), "then must be `b`");
                match else_expr.kind {
                    ExprKind::Assign { op: AssignOp::Eq, .. } => {}
                    other => panic!("else must be `c = d`, got {other:?}"),
                }
            }
            other => panic!("expected top-level Cond, got {other:?}"),
        }
    }

    #[test]
    fn conditional_missing_colon_emits_diagnostic_and_recovers() {
        // `a ? b c` — no `:`. The parser must emit a diagnostic that
        // mentions the conditional expression and label the `?` token,
        // then hand back *some* AST so downstream phases can keep
        // making progress. We accept either "return lhs" or "return a
        // Cond shell"; the stable contract is that a diagnostic is
        // emitted and the cursor does not spin.
        let src = "a ? b c";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = lex_ascii(fid, src);
        let tokens = convert(&mut sess, &pps);
        let total_tokens = tokens.len();
        let mut parser = Parser::new(&mut sess, tokens);
        let _ = parse_expression(&mut parser);
        let diags = cap.diagnostics();
        assert!(!diags.is_empty(), "missing `:` must diagnose");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("conditional expression") && d.message.contains(':')),
            "diagnostic must mention `:` and conditional expression, got {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>(),
        );
        // Parser must not have gone past the token stream or spun.
        assert!(parser.cursor <= total_tokens, "cursor must stay within token stream");
    }

    #[test]
    fn conditional_span_covers_full_range() {
        // Span must stretch from the cond operand's start to the
        // else operand's end — used by later diagnostics to underline
        // the whole ternary cleanly.
        let src = "a ? b : c";
        let (e, _cap) = parse_expr_str(src);
        assert_eq!(e.span.lo.0, 0, "span must start at `a`");
        assert_eq!(e.span.hi.0, src.len() as u32, "span must end at `c`");
    }

    // ── 05-12 comma operator (§6.5.17) ──────────────────────────────

    #[test]
    fn comma_is_left_associative_three_operands() {
        // Acceptance: `a, b, c` folds as `Comma(Comma(a, b), c)` —
        // the C99 grammar `expression: expression , assignment-expr`
        // is left-recursive, so every new `,` wraps the prefix
        // already built.
        let (e, cap) = parse_expr_str("a, b, c");
        assert!(cap.diagnostics().is_empty(), "valid `,` must be diag-free");
        match e.kind {
            ExprKind::Comma { lhs, rhs } => {
                // Outer rhs is the rightmost operand `c`.
                assert!(matches!(rhs.kind, ExprKind::Ident(_)), "outer rhs must be `c`");
                // Outer lhs is the inner `Comma(a, b)`.
                match lhs.kind {
                    ExprKind::Comma { lhs: inner_l, rhs: inner_r } => {
                        assert!(matches!(inner_l.kind, ExprKind::Ident(_)));
                        assert!(matches!(inner_r.kind, ExprKind::Ident(_)));
                    }
                    other => panic!("outer lhs must be inner Comma, got {other:?}"),
                }
            }
            other => panic!("expected top-level Comma, got {other:?}"),
        }
    }

    #[test]
    fn call_arguments_are_not_comma_folded() {
        // Acceptance: `f(a, b)` yields `Call { args: [a, b] }`, NOT
        // `Call { args: [Comma(a, b)] }`. This pins down §6.5.2.2 ¶1
        // (each argument is an *assignment-expression*) now that the
        // Pratt loop folds `,` at lower precedence.
        let (e, cap) = parse_expr_str("f(a, b)");
        assert!(cap.diagnostics().is_empty());
        match e.kind {
            ExprKind::Call { args, .. } => {
                assert_eq!(args.len(), 2, "must be two separate arguments");
                assert!(
                    !matches!(args[0].kind, ExprKind::Comma { .. }),
                    "arg 0 must not be a Comma node",
                );
                assert!(matches!(args[0].kind, ExprKind::Ident(_)));
                assert!(matches!(args[1].kind, ExprKind::Ident(_)));
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn call_with_three_arguments_has_three_slots() {
        // Extension of the two-arg check: `f(a, b, c)` must land
        // exactly three args in the vector, not a nested
        // `Comma(Comma(a, b), c)` smuggled into arg 0.
        let (e, _cap) = parse_expr_str("f(a, b, c)");
        match e.kind {
            ExprKind::Call { args, .. } => {
                assert_eq!(args.len(), 3);
                for a in &args {
                    assert!(matches!(a.kind, ExprKind::Ident(_)));
                }
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn parenthesised_comma_inside_call_arg_reaches_comma_operator() {
        // Acceptance: `f((a, b))` — the outer `(a, b)` is a
        // parenthesised expression, and the parenthesised-expression
        // production is *expression* (§6.5.1), not
        // assignment-expression. So the inner `,` *does* fold into a
        // `Comma` node, wrapped in a `Paren`, and becomes argument 0
        // of a one-arg call.
        let (e, cap) = parse_expr_str("f((a, b))");
        assert!(cap.diagnostics().is_empty());
        match e.kind {
            ExprKind::Call { args, .. } => {
                assert_eq!(args.len(), 1, "parenthesised comma counts as one arg");
                match &args[0].kind {
                    ExprKind::Paren(inner) => match &inner.kind {
                        ExprKind::Comma { lhs, rhs } => {
                            assert!(matches!(lhs.kind, ExprKind::Ident(_)));
                            assert!(matches!(rhs.kind, ExprKind::Ident(_)));
                        }
                        other => panic!("inside Paren must be Comma, got {other:?}"),
                    },
                    other => panic!("arg 0 must be Paren, got {other:?}"),
                }
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn comma_binds_weaker_than_assignment() {
        // Acceptance: `a = b, c = d` parses as
        // `Comma(Assign(a, b), Assign(c, d))` — assignment's
        // `(2, 1)` bp is strictly above comma's `(0, 1)`, so each
        // assignment folds fully before the `,` is even considered.
        let (e, _cap) = parse_expr_str("a = b, c = d");
        match e.kind {
            ExprKind::Comma { lhs, rhs } => {
                match lhs.kind {
                    ExprKind::Assign { op: AssignOp::Eq, .. } => {}
                    other => panic!("lhs must be `a = b`, got {other:?}"),
                }
                match rhs.kind {
                    ExprKind::Assign { op: AssignOp::Eq, .. } => {}
                    other => panic!("rhs must be `c = d`, got {other:?}"),
                }
            }
            other => panic!("expected top-level Comma, got {other:?}"),
        }
    }

    #[test]
    fn comma_span_covers_both_operands() {
        // The folded node's span must stretch from the leftmost
        // operand's start to the rightmost's end — same invariant
        // as every other binary node; downstream diagnostics rely
        // on it to underline the whole sequence.
        let src = "a, b";
        let (e, _cap) = parse_expr_str(src);
        assert!(matches!(e.kind, ExprKind::Comma { .. }));
        assert_eq!(e.span.lo.0, 0, "span must start at `a`");
        assert_eq!(e.span.hi.0, src.len() as u32, "span must end at `b`");
    }

    #[test]
    fn parse_assignment_expression_stops_at_top_level_comma() {
        // Direct contract test for the second entry point: calling
        // [`parse_assignment_expression`] on `a, b` must return just
        // `a` (an `Ident`) and leave the cursor on the `,` so the
        // caller can dispatch it as a separator. This is exactly
        // what `parse_call_args` relies on.
        let src = "a, b";
        let (mut sess, fid, cap) = mk_session(src);
        let pps = lex_ascii(fid, src);
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let e = parse_assignment_expression(&mut parser).expect("`a` parses");
        assert!(matches!(e.kind, ExprKind::Ident(_)), "must stop at `,`");
        // Cursor is now on the `,`.
        let at_comma =
            matches!(parser.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Comma)),);
        assert!(at_comma, "cursor must point at `,`, peek is {:?}", parser.peek().map(|t| &t.kind));
        assert!(cap.diagnostics().is_empty(), "clean stop, no diagnostics");
    }

    // ── Cast and sizeof (C99 §6.5.4 / §6.5.3.4) ─────────────────────
    //
    // These tests need the *real* tokenizer because they feed
    // multi-char keywords (`int`, `sizeof`) and the `lex_ascii`
    // mini-lexer used elsewhere only emits one-byte idents / digits.
    //
    // The helper mirrors the one in `decl::tests`: tokenize, run
    // phase-7 conversion, build a parser, optionally declare some
    // typedef symbols in the scope stack, then parse a full
    // expression.

    use crate::scope::NameKind;
    use rcc_ast::{Initializer, OffsetofDesignator, RecordKind, TypeSpec};
    use rcc_lexer::Tokenizer;

    fn parse_expr_full(
        src: &str,
        typedefs: &[&str],
    ) -> (Expr, Vec<rcc_errors::Diagnostic>, Session) {
        let (mut sess, fid, cap) = mk_session(src);
        let pps: Vec<PpToken> = Tokenizer::new(fid, src).collect();
        let tokens = convert(&mut sess, &pps);
        let typedef_syms: Vec<_> = typedefs.iter().map(|name| sess.interner.intern(name)).collect();
        let mut parser = Parser::new(&mut sess, tokens);
        for sym in typedef_syms {
            parser.scopes.declare(sym, NameKind::Typedef);
        }
        let e = parse_expression(&mut parser).expect("expression parses");
        (e, cap.diagnostics(), sess)
    }

    #[test]
    fn cast_int_of_ident_parses() {
        // `(int)x` — the canonical cast shape. The `(` is followed by
        // the `int` keyword, so `starts_type_name_after_lparen` fires
        // and `parse_cast` builds `ExprKind::Cast { ty: int, expr:
        // Ident(x) }`. Span must cover the whole `(int)x` sequence.
        let src = "(int)x";
        let (e, diags, sess) = parse_expr_full(src, &[]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        match &e.kind {
            ExprKind::Cast { ty, expr } => {
                assert!(matches!(ty.specs.type_specs.as_slice(), [TypeSpec::Int]));
                assert!(ty.declarator.name.is_none());
                assert!(ty.declarator.derived.is_empty());
                match &expr.kind {
                    ExprKind::Ident(sym) => assert_eq!(sess.interner.get(*sym), "x"),
                    other => panic!("expected Ident(x) operand, got {other:?}"),
                }
            }
            other => panic!("expected Cast, got {other:?}"),
        }
        assert_eq!(e.span.lo.0, 0);
        assert_eq!(e.span.hi.0 as usize, src.len());
    }

    #[test]
    fn sizeof_ident_parses_to_sizeof_expr() {
        // `sizeof x` — unary-expression form (no parens). Produces
        // `ExprKind::SizeofExpr(Ident(x))`.
        let src = "sizeof x";
        let (e, diags, sess) = parse_expr_full(src, &[]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        match &e.kind {
            ExprKind::SizeofExpr(inner) => match &inner.kind {
                ExprKind::Ident(sym) => assert_eq!(sess.interner.get(*sym), "x"),
                other => panic!("expected Ident(x), got {other:?}"),
            },
            other => panic!("expected SizeofExpr, got {other:?}"),
        }
    }

    #[test]
    fn sizeof_parenthesised_ident_stays_sizeof_expr() {
        // `sizeof (x)` — the token past `(` is an ordinary ident that
        // isn't a typedef-name, so we fall through to the unary-
        // expression form. The inner `(x)` parses as a paren-
        // expression and the whole thing becomes
        // `SizeofExpr(Paren(Ident(x)))`. This is regression-critical:
        // an overzealous type-name probe would wrongly pick the
        // `SizeofType` branch and fail to parse `x` as a type.
        let src = "sizeof(x)";
        let (e, diags, sess) = parse_expr_full(src, &[]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        match &e.kind {
            ExprKind::SizeofExpr(inner) => match &inner.kind {
                ExprKind::Paren(p) => match &p.kind {
                    ExprKind::Ident(sym) => assert_eq!(sess.interner.get(*sym), "x"),
                    other => panic!("expected Ident(x), got {other:?}"),
                },
                other => panic!("expected Paren(Ident(x)), got {other:?}"),
            },
            other => panic!("expected SizeofExpr, got {other:?}"),
        }
    }

    #[test]
    fn sizeof_type_int_parses() {
        // `sizeof(int)` — token past `(` is `int`, a type-specifier
        // keyword, so we take the `SizeofType` branch.
        let src = "sizeof(int)";
        let (e, diags, _sess) = parse_expr_full(src, &[]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        match &e.kind {
            ExprKind::SizeofType(ty) => {
                assert!(matches!(ty.specs.type_specs.as_slice(), [TypeSpec::Int]));
                assert!(ty.declarator.name.is_none());
                assert!(ty.declarator.derived.is_empty());
            }
            other => panic!("expected SizeofType, got {other:?}"),
        }
        assert_eq!(e.span.lo.0, 0);
        assert_eq!(e.span.hi.0 as usize, src.len());
    }

    #[test]
    fn sizeof_type_pointer_parses() {
        // `sizeof(int*)` — pointer abstract declarator inside the
        // type-name. The `TypeName::declarator` carries a single
        // `[Pointer]` derivation.
        let src = "sizeof(int*)";
        let (e, diags, _sess) = parse_expr_full(src, &[]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        match &e.kind {
            ExprKind::SizeofType(ty) => {
                assert!(matches!(ty.specs.type_specs.as_slice(), [TypeSpec::Int]));
                assert!(
                    matches!(
                        ty.declarator.derived.as_slice(),
                        [rcc_ast::DerivedDeclarator::Pointer(_)]
                    ),
                    "expected [Pointer], got {:?}",
                    ty.declarator.derived
                );
            }
            other => panic!("expected SizeofType, got {other:?}"),
        }
    }

    #[test]
    fn typedef_name_disambiguates_to_cast() {
        // `typedef int T; (T)x` — with `T` declared as a typedef-name
        // in the enclosing scope, `(T)` must parse as a cast's type-
        // name, NOT as a parenthesised expression. Mirrors the
        // §6.7.2p2 footnote: without the typedef classification the
        // grammar is ambiguous.
        let src = "(T)x";
        let (e, diags, sess) = parse_expr_full(src, &["T"]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        match &e.kind {
            ExprKind::Cast { ty, expr } => {
                match ty.specs.type_specs.as_slice() {
                    [TypeSpec::TypedefName(sym)] => {
                        assert_eq!(sess.interner.get(*sym), "T");
                    }
                    other => panic!("expected TypedefName(T), got {other:?}"),
                }
                match &expr.kind {
                    ExprKind::Ident(sym) => assert_eq!(sess.interner.get(*sym), "x"),
                    other => panic!("expected Ident(x) operand, got {other:?}"),
                }
            }
            other => panic!("expected Cast, got {other:?}"),
        }
    }

    #[test]
    fn ordinary_name_disambiguates_to_paren() {
        // `int T = 0; (T)` — with `T` NOT a typedef-name, `(T)` must
        // parse as a parenthesised expression wrapping `Ident(T)`.
        // The expression stops there; typeck / name-resolution will
        // later decide what `T` actually refers to. This exercises
        // the "NOT typedef → fall through to parse_primary" arm.
        //
        // We don't pre-declare `T` in the scope stack (or declare it
        // as `Ordinary`); either way `is_typedef(T)` is false.
        let src = "(T)";
        let (e, diags, sess) = parse_expr_full(src, &[]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        match &e.kind {
            ExprKind::Paren(inner) => match &inner.kind {
                ExprKind::Ident(sym) => assert_eq!(sess.interner.get(*sym), "T"),
                other => panic!("expected Ident(T), got {other:?}"),
            },
            other => panic!("expected Paren, got {other:?}"),
        }
    }

    #[test]
    fn cast_nests_into_further_cast_expression() {
        // `(int)(long)x` — cast-expression recurses into cast-
        // expression (§6.5.4), not into unary-expression. The outer
        // Cast's operand must itself be a Cast, not a Paren.
        let src = "(int)(long)x";
        let (e, diags, sess) = parse_expr_full(src, &[]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        match &e.kind {
            ExprKind::Cast { ty, expr } => {
                assert!(matches!(ty.specs.type_specs.as_slice(), [TypeSpec::Int]));
                match &expr.kind {
                    ExprKind::Cast { ty: inner, expr: inner_expr } => {
                        assert!(matches!(inner.specs.type_specs.as_slice(), [TypeSpec::Long]));
                        match &inner_expr.kind {
                            ExprKind::Ident(sym) => assert_eq!(sess.interner.get(*sym), "x"),
                            other => panic!("expected Ident(x), got {other:?}"),
                        }
                    }
                    other => panic!("expected inner Cast, got {other:?}"),
                }
            }
            other => panic!("expected outer Cast, got {other:?}"),
        }
    }

    #[test]
    fn cast_of_negated_expression_parses() {
        // `(int)-x` — the cast operand is itself a unary-expression
        // (`-x`), not a primary. This guards the fall-through from
        // `parse_cast` into `parse_prefix_unary`, which must pick up
        // the `-` before recursing into the postfix chain.
        let src = "(int)-x";
        let (e, diags, _sess) = parse_expr_full(src, &[]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        match &e.kind {
            ExprKind::Cast { expr, .. } => match &expr.kind {
                ExprKind::Unary { op: UnOp::Neg, .. } => {}
                other => panic!("expected Unary(Neg), got {other:?}"),
            },
            other => panic!("expected Cast, got {other:?}"),
        }
    }

    #[test]
    fn sizeof_is_right_associative_with_nested_sizeof() {
        // `sizeof sizeof x` — two nested sizeof-expressions. The
        // outer `sizeof` consumes the rest as a unary-expression,
        // which itself begins with a `sizeof`. Tree shape:
        // SizeofExpr(SizeofExpr(Ident(x))).
        let src = "sizeof sizeof x";
        let (e, diags, _sess) = parse_expr_full(src, &[]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        match &e.kind {
            ExprKind::SizeofExpr(outer) => match &outer.kind {
                ExprKind::SizeofExpr(inner) => match &inner.kind {
                    ExprKind::Ident(_) => {}
                    other => panic!("expected Ident, got {other:?}"),
                },
                other => panic!("expected nested SizeofExpr, got {other:?}"),
            },
            other => panic!("expected SizeofExpr, got {other:?}"),
        }
    }

    // ── Compound literals (C99 §6.5.2.5) ────────────────────────────

    #[test]
    fn compound_literal_int_array() {
        // `(int[3]){0}` — compound literal with array type and single
        // positional initializer.
        let src = "(int[3]){0}";
        let (e, diags, sess) = parse_expr_full(src, &[]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        match &e.kind {
            ExprKind::CompoundLiteral { ty, init } => {
                assert_eq!(ty.specs.type_specs.len(), 1);
                assert!(matches!(ty.specs.type_specs[0], TypeSpec::Int));
                assert!(!ty.declarator.derived.is_empty(), "must have array declarator");
                match init.as_ref() {
                    Initializer::List(items) => {
                        assert_eq!(items.len(), 1);
                        let (desig, sub) = &items[0];
                        assert!(desig.is_empty(), "positional element");
                        match sub {
                            Initializer::Expr(inner) => {
                                assert_eq!(
                                    sess.interner.get(match &inner.kind {
                                        ExprKind::IntLit(lit) => lit.text,
                                        other => panic!("expected IntLit, got {other:?}"),
                                    }),
                                    "0"
                                );
                            }
                            other => panic!("expected Expr, got {other:?}"),
                        }
                    }
                    other => panic!("expected List, got {other:?}"),
                }
            }
            other => panic!("expected CompoundLiteral, got {other:?}"),
        }
    }

    #[test]
    fn compound_literal_struct_with_designator() {
        // `(struct S){.x = 1}` — compound literal with struct type
        // and field designator.
        let src = "(struct S){.x = 1}";
        let (e, diags, _sess) = parse_expr_full(src, &[]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        match &e.kind {
            ExprKind::CompoundLiteral { ty, init } => {
                assert!(
                    matches!(ty.specs.type_specs[0], TypeSpec::Record(_)),
                    "must be struct type"
                );
                match init.as_ref() {
                    Initializer::List(items) => {
                        assert_eq!(items.len(), 1);
                        assert_eq!(items[0].0.len(), 1, "one designator");
                    }
                    other => panic!("expected List, got {other:?}"),
                }
            }
            other => panic!("expected CompoundLiteral, got {other:?}"),
        }
    }

    #[test]
    fn compound_literal_postfix_member() {
        // `((T){0}).x` — postfix `.x` on a compound literal.
        // Requires typedef `T` in scope.
        let src = "((T){0}).x";
        let (e, diags, _sess) = parse_expr_full(src, &["T"]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        match &e.kind {
            ExprKind::Member { base, field } => {
                assert_eq!(_sess.interner.get(*field), "x");
                match &base.kind {
                    ExprKind::Paren(inner) => match &inner.kind {
                        ExprKind::CompoundLiteral { .. } => {}
                        other => panic!("expected CompoundLiteral inside paren, got {other:?}"),
                    },
                    other => panic!("expected Paren, got {other:?}"),
                }
            }
            other => panic!("expected Member, got {other:?}"),
        }
    }

    #[test]
    fn cast_still_parses_after_compound_literal_addition() {
        // Regression: `(int)x` must still parse as Cast, not
        // CompoundLiteral, since `x` does not start with `{`.
        let src = "(int)x";
        let (e, diags, _sess) = parse_expr_full(src, &[]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        assert!(matches!(e.kind, ExprKind::Cast { .. }), "expected Cast, got {:?}", e.kind);
    }

    #[test]
    fn sizeof_type_still_parses_after_compound_literal_addition() {
        // Regression: `sizeof(int)` must still parse as SizeofType.
        let src = "sizeof(int)";
        let (e, diags, _sess) = parse_expr_full(src, &[]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        assert!(matches!(e.kind, ExprKind::SizeofType(_)), "expected SizeofType, got {:?}", e.kind);
    }

    #[test]
    fn builtin_offsetof_type_argument_parses() {
        let src = "__builtin_offsetof(struct S, x)";
        let (e, diags, sess) = parse_expr_full(src, &[]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        match &e.kind {
            ExprKind::BuiltinOffsetof { ty, designators } => {
                match ty.specs.type_specs.as_slice() {
                    [TypeSpec::Record(rec)] => {
                        assert_eq!(rec.kind, RecordKind::Struct);
                        let tag = rec.tag.expect("struct tag");
                        assert_eq!(sess.interner.get(tag), "S");
                    }
                    other => panic!("expected struct S type-name, got {other:?}"),
                }
                assert_eq!(designators.len(), 1);
                match &designators[0] {
                    OffsetofDesignator::Field(field) => {
                        assert_eq!(sess.interner.get(*field), "x");
                    }
                    other => panic!("expected field designator, got {other:?}"),
                }
            }
            other => panic!("expected BuiltinOffsetof, got {other:?}"),
        }
    }

    #[test]
    fn builtin_offsetof_nested_fields_and_subscript_parse() {
        let src = "__builtin_offsetof(struct S, a[2].b)";
        let (e, diags, sess) = parse_expr_full(src, &[]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        match &e.kind {
            ExprKind::BuiltinOffsetof { designators, .. } => {
                assert_eq!(designators.len(), 3);
                match &designators[0] {
                    OffsetofDesignator::Field(field) => {
                        assert_eq!(sess.interner.get(*field), "a");
                    }
                    other => panic!("expected first field, got {other:?}"),
                }
                match &designators[1] {
                    OffsetofDesignator::Index(index) => match &index.kind {
                        ExprKind::IntLit(lit) => assert_eq!(lit.value, 2),
                        other => panic!("expected integer subscript, got {other:?}"),
                    },
                    other => panic!("expected index designator, got {other:?}"),
                }
                match &designators[2] {
                    OffsetofDesignator::Field(field) => {
                        assert_eq!(sess.interner.get(*field), "b");
                    }
                    other => panic!("expected final field, got {other:?}"),
                }
            }
            other => panic!("expected BuiltinOffsetof, got {other:?}"),
        }
    }

    #[test]
    fn builtin_types_compatible_typedef_names_parse() {
        let src = "__builtin_types_compatible_p(T, int *)";
        let (e, diags, sess) = parse_expr_full(src, &["T"]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        match &e.kind {
            ExprKind::BuiltinTypesCompatible { lhs, rhs } => {
                match lhs.specs.type_specs.as_slice() {
                    [TypeSpec::TypedefName(sym)] => assert_eq!(sess.interner.get(*sym), "T"),
                    other => panic!("expected typedef-name lhs, got {other:?}"),
                }
                assert!(matches!(rhs.specs.type_specs.as_slice(), [TypeSpec::Int]));
                assert!(
                    matches!(
                        rhs.declarator.derived.as_slice(),
                        [rcc_ast::DerivedDeclarator::Pointer(_)]
                    ),
                    "expected pointer rhs, got {:?}",
                    rhs.declarator.derived
                );
            }
            other => panic!("expected BuiltinTypesCompatible, got {other:?}"),
        }
    }

    #[test]
    fn ordinary_expression_builtin_remains_call() {
        let src = "__builtin_expect(x, 1)";
        let (e, diags, sess) = parse_expr_full(src, &[]);
        assert!(diags.is_empty(), "clean: {diags:?}");
        match &e.kind {
            ExprKind::Call { callee, args } => {
                assert_eq!(args.len(), 2);
                match &callee.kind {
                    ExprKind::Ident(sym) => assert_eq!(sess.interner.get(*sym), "__builtin_expect"),
                    other => panic!("expected builtin callee ident, got {other:?}"),
                }
            }
            other => panic!("expected ordinary Call, got {other:?}"),
        }
    }

    #[test]
    fn builtin_malformed_type_argument_diagnoses() {
        let src = "__builtin_types_compatible_p(static int, long)";
        let (e, diags, _sess) = parse_expr_full(src, &[]);
        assert!(
            diags.iter().any(|d| d.code == Some("E0061")),
            "expected strict type-name diagnostic, got {diags:?}"
        );
        assert!(matches!(e.kind, ExprKind::BuiltinTypesCompatible { .. }));
    }

    #[test]
    fn builtin_offsetof_malformed_member_diagnoses() {
        let src = "__builtin_offsetof(struct S, 1)";
        let (e, diags, _sess) = parse_expr_full(src, &[]);
        assert!(!diags.is_empty(), "expected diagnostics");
        match &e.kind {
            ExprKind::BuiltinOffsetof { designators, .. } => {
                assert!(designators.is_empty(), "bad member should not fabricate designator");
            }
            other => panic!("expected BuiltinOffsetof, got {other:?}"),
        }
    }
}
