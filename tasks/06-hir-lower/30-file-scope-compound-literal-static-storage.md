> ✓ done — 2026-05-04

# 06-30: file-scope compound literal static storage

**Phase:** 06-hir-lower    **Depends on:** 06-21, 07-16, 09-11    **Milestone:** M6+

## Goal
Materialize file-scope compound literals as anonymous internal globals with
static storage duration so their address is a valid static initializer.

## Trigger
- `c-testsuite::00149` rejects `struct S *s = &(struct S){1, 2};`.
- `c-testsuite::00150` rejects a nested file-scope compound literal used as a
  global pointer initializer.

## Scope
- In:
  - During HIR lowering, turn a file-scope compound literal into a synthetic
    internal global with a `GlobalInit`.
  - Lower `&compound_literal` in a global initializer to an address constant
    referring to that synthetic global.
  - Preserve designated and nested initializer semantics.
  - Ensure LLVM codegen emits the synthetic object before users of its address.
- Out:
  - GNU compound literal extensions outside C99.

## Deliverables
- HIR/typeck/codegen tests for `&(struct S){...}` at file scope.
- c-testsuite regressions for `00149` and `00150`.

## Acceptance
- `c-testsuite::00149` and `c-testsuite::00150` no longer emit `E0084`.
- The generated executable observes the initialized fields correctly.

## References
- C99 §6.5.2.5p5
- `third_party/testsuites/c-testsuite/tests/single-exec/00149.c`
- `third_party/testsuites/c-testsuite/tests/single-exec/00150.c`
