> ✓ done — 2026-04-29

# 08-05: Short-circuit operators

**Phase:** 08-cfg    **Depends on:** 08-03    **Milestone:** M3

## Goal
Lower `a && b`, `a || b`, `a ? b : c` into branch structures in the
CFG. Result is a fresh temporary assigned in both branches, joined in
a successor block.

## Scope
- In: emit 3 blocks (rhs, join) for `&&`/`||`; 4 blocks (then, else,
  join) for `?:`.
- Out: --.

## Deliverables
- Lowering helpers.
- MIR snapshot: `a && b`.

## Acceptance
- Lowered CFG visits `rhs` block only when `a` is non-zero for `&&`.
- Exit block reads the temp with a `LvalueToRvalue` load.

## References
- rustc MIR "short-circuit" lowering.
