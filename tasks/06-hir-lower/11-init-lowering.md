# 06-11: Initializer lowering

**Phase:** 06-hir-lower    **Depends on:** 06-06, 06-10    **Milestone:** M4

## Goal
Flatten `Initializer::List` into a sequence of `HirStmtKind::Expr`
assignments (for locals) or constant data (for globals) per C99
§6.7.8. Designated initializers are resolved against the destination
`Ty`.

## Scope
- In: an "initializer walker" that pairs each value with its target
  offset / field index; zero-fill gaps (C99 §6.7.8p21).
- Out: constant-fold for globals → `ConstKind::Int` etc. (codegen).

## Deliverables
- `lower_initializer(target_ty, init, body) -> Vec<HirStmt>`.
- Tests from §6.7.8 examples 1, 11, 17.

## Acceptance
- `int a[3] = {1}` lowers to three stores: `a[0]=1`, `a[1]=0`,
  `a[2]=0` (or equivalent zero-fill marker).
- Nested designators `{ .x[1] = 5 }` resolve correctly.

## References
- C99 §6.7.8.
