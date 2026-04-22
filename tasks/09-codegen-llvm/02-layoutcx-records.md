# 09-02: Record layout

**Phase:** 09-codegen-llvm    **Depends on:** 09-01    **Milestone:** M4

## Goal
Compute struct / union layout with padding:
- **struct** — fields placed at the first offset satisfying
  alignment, size rounded up to alignment, overall alignment is max
  of field alignments.
- **union** — all fields at offset 0, size = max field size, align
  = max field align.
- **bitfield** — pack into underlying storage type; zero-width
  bitfield separates subsequent ones into the next storage unit.

## Scope
- In: cache layouts by `DefId` (use `Field::offset`); store on
  `DefKind::Record`.
- Out: C11 `_Alignas` (not in C99).

## Deliverables
- Layout builder; populates `Field::offset` + `Layout` on the record.
- Tests including the GNU-ABI "struct with trailing padding" corner.

## Acceptance
- `struct { char a; int b; }`: `a@0`, `b@4`, size 8, align 4.
- `struct { char a; char b; int c; }`: `a@0`, `b@1`, `c@4`, size 8.

## References
- System V ABI, section 3.1.2 "Aggregates and Unions".
