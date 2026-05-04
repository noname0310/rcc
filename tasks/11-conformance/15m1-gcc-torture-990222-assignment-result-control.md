# 11-15m1: gcc-torture 990222 assignment-result control

**Phase:** 11-conformance    **Depends on:** 11-15m    **Milestone:** M6

## Goal
Fix the C99 runtime bug behind `gcc-torture::execute::990222-1`, or reduce it
to a narrower checked task if the abort-shaped failure proves to be in the
conformance runner rather than rcc-generated code.

## Scope
- In: `(*--ptr += 1) > '9'` as a while condition, assignment-expression result
  value, pointer predecrement sequencing, and nested `||` control-flow lowering.
- In: call/branch shape where the false path skips `abort()`.
- Out: unrelated libc `abort` semantics and GNU-only cases.

## Deliverables
- A reduced runtime fixture that distinguishes:
  - return-code version of the `line` update,
  - original `if (... || ... || ...) abort();` shape,
  - direct CFG/LLVM branch result for the nested logical-OR condition.
- A code fix when the reduced fixture proves rcc lowers the condition or
  assignment result incorrectly.

## Acceptance
- `gcc-torture::execute::990222-1` passes under WSL LLVM, or the remaining
  failure is proven to be non-rcc with a checked runner/tooling task.
- No xfail, skip, or result masking is added.

## References
- `tasks/11-conformance/15m-gcc-torture-scalar-conversion-cluster.md`
