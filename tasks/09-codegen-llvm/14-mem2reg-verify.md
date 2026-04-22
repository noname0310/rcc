# 09-14: `mem2reg` verification

**Phase:** 09-codegen-llvm    **Depends on:** 09-07    **Milestone:** M3

## Goal
After function codegen, run `opt -mem2reg` equivalent via
`PassManager` and assert no `alloca` remains for a function whose
locals are all "promotable" (no address-taken, scalar type). This
proves the skeleton's "lean on mem2reg" design works in practice.

## Scope
- In: use inkwell's `PassManagerBuilder` or manual `FunctionPassManager`;
  test-only fixture asserts post-pass instruction counts.
- Out: running the full `-O2` pipeline (driver task).

## Deliverables
- Post-codegen verification harness in tests.
- Fixture confirming `int f(int x){ int y = x+1; return y; }` has
  zero `alloca` after mem2reg.

## Acceptance
- On the fixture: exactly 0 `alloca` instructions after pass.
- On a fixture with `&y`: exactly 1 `alloca` remains.

## References
- LLVM `mem2reg` pass docs.
