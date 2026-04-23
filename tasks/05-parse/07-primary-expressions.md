> ✓ done — 2026-04-23

# 05-07: Primary expressions

**Phase:** 05-parse    **Depends on:** 05-03 .. 05-06    **Milestone:** M1+

## Goal
Parse C99 §6.5.1 primary-expression: `identifier`, integer-const,
float-const, char-const, string-literal, `( expression )`.

## Scope
- In: `parse_primary()` dispatching on the current token kind;
  production returns `Expr` with matching `ExprKind`.
- Out: postfix trailers (task 09).

## Deliverables
- `parse_primary` fn in `crates/rcc_parse/src/expr.rs`.
- Unit tests: one per arm.

## Acceptance
- Parsing `(42)` yields `ExprKind::Paren(Expr::IntLit {..})`.
- Unknown identifier still parses as `ExprKind::Ident(sym)` (name
  resolution is in HIR lowering).

## References
- C99 §6.5.1.
