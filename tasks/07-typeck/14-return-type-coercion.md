> ✓ done — 2026-05-01

# 07-14: Return type coercion and diagnostics

**Phase:** 07-typeck    **Depends on:** 07-13    **Milestone:** M3 pre-codegen stabilization

## Goal
Type-check `return` statements against the enclosing function return
type. LLVM codegen must never receive a return-slot store whose source
value is not assignable to the declared return type.

## Scope
- In: thread the enclosing function's return type through the statement
  visitor.
- In: apply the same assignment-compatible coercion used for local
  initializers and simple assignments.
- In: diagnose `return expr;` in `void` functions and bare `return;` in
  non-`void` functions.
- In: preserve existing real/complex conversion warning behavior.
- Out: control-flow analysis for missing return on all paths.

## Deliverables
- `check_body_with_defs` or a new crate-level context that knows the
  function `DefId` / return type.
- Rewired `HirStmtKind::Return` expression ids when conversions are
  inserted.
- UI/unit tests for arithmetic, pointer, struct, void, and complex
  return cases.

## Acceptance
- `long f(void) { int x; return x; }` inserts a conversion to `long`.
- `void f(void) { return 1; }` emits an error.
- `int f(void) { return; }` emits an error.
- `struct A f(struct B b) { return b; }` emits an incompatible return
  type diagnostic.

## References
- C99 §6.8.6.4 — return statement.
- `crates/rcc_typeck/src/lib.rs`, current comment saying return type is
  not threaded.
