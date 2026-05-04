# 11-15l: gcc-torture bit-field precision cluster

**Phase:** 11-conformance    **Depends on:** 11-15k    **Milestone:** M6

## Goal
Fix or further split the remaining bit-field runtime aborts after explicit
bit-field storage layout was introduced.

## Scope
- In: `bf-sign-2`, `bitfld-1`, `bitfld-3`, `bitfld-5`, `pr31448-2`,
  `pr32244-1`, `pr34971`, `pr58984`, `struct-ini-2`.
- Out: GNU `scalar_storage_order` and vector bit-field interactions.

## Deliverables
- Reduced fixtures for bit-field promotion, precision-truncating arithmetic,
  signed extraction, and stores into sub-`int` and wider-than-`int` fields.
- Typeck/codegen fixes or smaller follow-up tasks when the cluster splits.
- Runtime tests proving host `cc` and rcc agree for each fixed reduction.

## Acceptance
- At least three listed cases pass, or every remaining case is mapped to a
  narrower checked task.
- No xfail, skip, or result masking is added.

## References
- `docs/gcc-torture-signal-clusters.md`
