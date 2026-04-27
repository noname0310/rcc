> ✓ done — 2026-04-28

# 08-03: Expression → Rvalue

**Phase:** 08-cfg    **Depends on:** 08-02    **Milestone:** M3

## Goal
Lower a `HirExpr` into a sequence of statements culminating in a
`Rvalue` (value expression) or a `Place` (lvalue). `lower_as_rvalue`
and `lower_as_place` are the two entry points.

## Scope
- In: every `HirExprKind` arm; temporaries are named `_t<N>`;
  `Convert` nodes emit `Rvalue::Cast`.
- Out: short-circuit lowering (task 05).

## Deliverables
- `lower_as_rvalue(&mut BodyBuilder, &HirExpr) -> Operand`.
- `lower_as_place(&mut BodyBuilder, &HirExpr) -> Place`.
- Snapshot fixtures for each `HirExprKind`.

## Acceptance
- `a + b * c` emits a single temporary for `b*c` then an add.
- `*p` lowers to `Place { base: <p>, projection: [Deref] }`.

## References
- rustc `rustc_mir_build::build::expr`.
