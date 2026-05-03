> ✓ done — 2026-05-04

# 08-07: Loop lowering

**Phase:** 08-cfg    **Depends on:** 08-06    **Milestone:** M3

## Goal
Lower `while`, `do-while`, `for` into CFG with a dedicated `header`
block and `break` / `continue` targets stored in a per-function loop
stack.

## Scope
- In: `LoopCtx { cont_target, break_target }` pushed on entry, popped
  on exit; `continue` targets the header (or the step block for
  `for`); `break` targets the join.
- Out: --.

## Deliverables
- `lower_while`, `lower_do_while`, `lower_for`.
- Snapshot: `for (int i = 0; i < 10; i++) {}`.

## Acceptance
- `break` / `continue` land on the correct block in nested loops.

## References
- C99 §6.8.5.
