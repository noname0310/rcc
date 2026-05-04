# 11-15s4: GNU vector scalar and vector casts

**Phase:** 11-conformance    **Depends on:** 11-15s3    **Milestone:** M6

## Goal
Implement the vector casts needed by the 20050316 and simd-4 cases.

## Scope
- In: same-size scalar integer <-> vector bitcasts.
- In: same-size vector <-> vector bitcasts.
- In: integer/float vector conversions used by `20050316-2`.
- Out: target intrinsics and saturating conversions.

## Deliverables
- Typeck conversion rules for vector casts.
- CFG cast representation that distinguishes bitcast from element conversion.
- LLVM `bitcast`/conversion emission tests.
- Reduced fixtures from `20050316-1`, `20050316-2`, `20050316-3`, and `simd-4`.

## Acceptance
- Same-size vector/scalar casts preserve the source bytes.
- Signed/unsigned same-lane vector casts preserve bit patterns where required.
- Invalid vector casts emit a vector-specific diagnostic.

## References
- `docs/gnu-vector-design.md`
- `gcc-torture::execute::20050316-1`
- `gcc-torture::execute::20050316-2`
- `gcc-torture::execute::20050316-3`
- `gcc-torture::execute::simd-4`
