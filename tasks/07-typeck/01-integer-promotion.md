# 07-01: Integer promotion

**Phase:** 07-typeck    **Depends on:** —    **Milestone:** M3

## Goal
Harden `rcc_typeck::integer_promotion` (currently a simple rank
comparison) against every C99 §6.3.1.1 corner case: `_Bool`, `char`,
`short`, bitfields.

## Scope
- In: function already exists; extend to bitfield-typed values (rank
  determined by storage type, promotion to int if int can represent
  all values, else unsigned int).
- Out: selecting the integer type of an integer literal (task 03-04
  in phase 05 overlaps; the final pick is encoded in the literal's
  suffix).

## Deliverables
- Updated `integer_promotion(tcx, ty, bit_width: Option<u32>)`.
- Truth-table test for every rank.

## Acceptance
- `_Bool` → `int`.
- 3-bit `unsigned int` bitfield → `int` (since int holds [0, 2^31)).
- `long` stays `long`.

## References
- C99 §6.3.1.1.
