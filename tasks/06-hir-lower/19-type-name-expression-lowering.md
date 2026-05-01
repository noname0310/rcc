# 06-19: lower `TypeName` expressions without losing types

**Phase:** 06-hir-lower    **Depends on:** 06-18    **Milestone:** M5 stabilization

## Goal
Preserve AST `TypeName` information in HIR for casts, `sizeof(type)`,
and the type part of compound literals.

## Scope
- In: `(T)expr` lowers to `HirExprKind::Cast { to: T }`.
- In: `sizeof(T)` gets an explicit HIR representation, e.g.
  `HirExprKind::SizeofType(TyId)`.
- In: typeck, const-eval, and CFG lowering support the new HIR shape
  enough for source-to-MIR tests.
- Out: materializing compound literal storage; task 06-20 handles that.

## Deliverables
- A `lower_type_name` helper built on the central type service.
- HIR expression enum extended if needed.
- Removal of the historical `SizeofType -> IntConst(0)` placeholder
  test.
- Tests for typedefs, pointers, arrays, records, and enums in
  `TypeName`.

## Acceptance
- `sizeof(int)` is not lowered as `IntConst(0)`.
- `sizeof(struct S)` routes through the shared layout service and fails
  on incomplete records with E0085, not with a bogus zero.
- `(long)x` has destination type `long`, not operand type fallback.
- `(T *)0` works when `T` is a typedef.

## References
- C99 §6.5.3.4 — `sizeof`.
- C99 §6.5.4 — Cast operators.
- `lower_expr` currently ignores cast `ty` and placeholder-lowers
  `SizeofType`.

