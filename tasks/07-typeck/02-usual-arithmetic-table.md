> ✓ done — 2026-04-26

# 07-02: Usual Arithmetic Conversion table

**Phase:** 07-typeck    **Depends on:** 07-01    **Milestone:** M3

## Goal
Port the full C99 §6.3.1.8 ladder into `rcc_typeck::usual_arithmetic`.
The current skeleton handles the main cases; this task makes the
"same rank, one signed one unsigned" rules match the standard exactly.

## Scope
- In: §6.3.1.8 bullet list — long double / double / float / integer
  promotion / equal rank / signed-rank-above-unsigned-rank.
- Out: `_Complex` arithmetic (M7 quality).

## Deliverables
- Updated fn body with spec-citation comments.
- Truth-table test with all 13 × 13 scalar-type pairs.

## Acceptance
- `signed int op unsigned int` → `unsigned int`.
- `long op unsigned int` on x86-64 → `long` (because long can
  represent unsigned int values).

## References
- C99 §6.3.1.8.
