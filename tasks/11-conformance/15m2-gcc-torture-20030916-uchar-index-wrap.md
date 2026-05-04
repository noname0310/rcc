> ✓ done — 2026-05-04

# 11-15m2: gcc-torture 20030916 unsigned-char index wrap

**Phase:** 11-conformance    **Depends on:** 11-15m1    **Milestone:** M6

## Goal
Fix the C99 runtime bug behind `gcc-torture::execute::20030916-1`, which
depends on `unsigned char` compound-assignment wrapping before array indexing.

## Scope
- In: `unsigned char` compound `+=` / `-=` conversion back to the lhs type,
  integer promotion after the narrowed store, and use as an array subscript.
- In: global/local array stores reached through a wrapped `unsigned char` index.
- Out: signed overflow and out-of-range float-to-int conversions.

## Deliverables
- A reduced runtime fixture for:
  - `unsigned char i = 0x10; i += 0xe8;`
  - `x[i] = 0;`
  - `i -= 0xe7;`
- A code fix if CFG/codegen fails to narrow the compound assignment result
  before the next use.
- A WSL gcc-torture probe proving `20030916-1` passes.

## Acceptance
- `gcc-torture::execute::20030916-1` passes under WSL LLVM.
- The reduced fixture passes host `cc` and rcc.
- No xfail, skip, or result masking is added.

## References
- `tasks/11-conformance/15m-gcc-torture-scalar-conversion-cluster.md`
