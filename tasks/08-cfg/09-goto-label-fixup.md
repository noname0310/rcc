# 08-09: `goto` and label fixup

**Phase:** 08-cfg    **Depends on:** 08-01    **Milestone:** M3

## Goal
Resolve forward `goto` by a two-pass approach: first pass creates
empty blocks for each label, second pass emits `Goto` terminators
pointing at them. Backward gotos resolve eagerly.

## Scope
- In: label → `BasicBlockId` map per function; `goto X` emits a
  `Goto(bb)` possibly into a provisional block that gets its
  statements filled later.
- Out: --.

## Deliverables
- Fixup helpers.
- Fixture: Duff's device (forward goto crossing switch labels).

## Acceptance
- `void f() { goto end; end: return; }` lowers without diagnostics.
- Unknown label already caught in HIR phase — defensive assert here.

## References
- rustc MIR's label-block map.
