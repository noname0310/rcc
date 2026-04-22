# 08-02: Local allocation

**Phase:** 08-cfg    **Depends on:** 08-01    **Milestone:** M3

## Goal
Populate `Body::locals` in this order: local 0 = return slot (`ret_ty`),
locals 1..=N = parameters, then declared locals in source order, then
temporaries produced during lowering.

## Scope
- In: `BodyBuilder::local(ty, name) -> Local`; parameter allocation
  helper.
- Out: register allocation (LLVM).

## Deliverables
- Builder API + test.

## Acceptance
- `void f(int a, int b) { int c; }` yields locals `[ret:void, a, b, c]`.

## References
- rustc's `LocalDecl` convention.
