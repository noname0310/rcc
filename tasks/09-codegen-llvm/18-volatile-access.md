# 09-18: `volatile` access codegen

**Phase:** 09-codegen-llvm    **Depends on:** 09-08    **Milestone:** M5

## Goal
Emit `load volatile` and `store volatile` LLVM instructions when
accessing objects qualified with `volatile`. Ensure LLVM does not
optimise away or reorder volatile accesses.

## Scope
- In: detect `volatile` qualifier on the pointee type when
  generating load/store instructions. Set the `volatile` flag on
  the LLVM `LoadInst` / `StoreInst`. Applies to direct variable
  access, pointer dereference, and struct member access through a
  volatile-qualified path.
- Out: `volatile` semantics for `memcpy`/`memset` of volatile
  aggregates (defer).

## Deliverables
- Volatile flag propagation in load/store emission.
- Tests: verify IR contains `load volatile` / `store volatile`.

## Acceptance
- `volatile int x; x = 1; int y = x;` emits `store volatile` and
  `load volatile` in LLVM IR.
- `opt -O2` does not eliminate the volatile load/store.
- Non-volatile accesses remain non-volatile.

## References
- C99 §6.7.3¶6 — volatile semantics.
- LLVM Language Reference: `volatile` flag on memory instructions.
