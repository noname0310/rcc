> ✓ done — 2026-05-04

# 11-15n: gcc-torture VLA lifetime cluster

**Phase:** 11-conformance    **Depends on:** 11-15k    **Milestone:** M6

## Goal
Fix VLA storage lifetime and parameter-bound side effects exposed by runtime
gcc-torture aborts.

## Scope
- In: `20040811-1`, `vla-dealloc-1`, `pr77767`.
- Out: non-VLA alloca extensions.

## Deliverables
- Reduced tests for backward gotos across VLA declarations and labels inside
  blocks before VLA declarations.
- A reduced test for VLA parameter bound side effects (`a++`, `c++`).
- CFG/codegen fixes so stack restore and parameter adjustment preserve C99
  semantics.

## Acceptance
- The three listed cases pass or are split into narrower checked tasks.
- No VLA case is skipped due to runtime cost; use reduced fixtures for fast
  regression tests.

## Result
- `20040811-1` and `vla-dealloc-1` pass after CFG emits `StorageDead` for
  locals whose lifetime starts after the target label, and LLVM stores VLA
  stack tokens in entry-block slots so branch-local `StorageDead` blocks can
  restore them path-independently.
- `pr77767` is split to `11-15n1` because it is a separate HIR/function
  parameter lowering issue: adjusted VLA parameter bounds with side effects
  (`a++`, `c++`) are currently dropped.

## References
- `docs/gcc-torture-signal-clusters.md`
