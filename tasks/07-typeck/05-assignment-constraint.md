> ✓ done — 2026-04-26

# 07-05: Assignment constraint

**Phase:** 07-typeck    **Depends on:** 07-04    **Milestone:** M3

## Goal
Check C99 §6.5.16.1 constraints on `=` (and hence on function call
args, returns, and initializers — all follow the same rule). Emit
E0081 "incompatible types in assignment".

## Scope
- In: compatibility predicate for scalar / pointer / struct / union;
  handle `void*` ↔ `T*` (§6.5.16.1p1 bullet 3).
- Out: detailed diagnostic messaging (covered by task 07).

## Deliverables
- `is_assignable(dst: TyId, src_expr: &HirExpr, tcx) -> Result<(), Diagnostic>`.
- Fixtures for every bullet of §6.5.16.1p1.

## Acceptance
- `int x = 1.5;` is accepted but emits W0007 (narrowing).
- `int *p = 0;` accepted (null pointer constant).
- `struct A; struct B; struct A a; struct B *p = &a;` → E0081.

## References
- C99 §6.5.16.1.
