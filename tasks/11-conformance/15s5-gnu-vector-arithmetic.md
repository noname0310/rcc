# 11-15s5: GNU vector arithmetic and scalar splats

**Phase:** 11-conformance    **Depends on:** 11-15s4    **Milestone:** M6

## Goal
Implement elementwise vector arithmetic and scalar-vector splats.

## Scope
- In: `+`, `-`, `*`, `/`, `%`, `^`, `&`, `|`, `<<`, `>>`.
- In: vector-vector and scalar-vector binary operations.
- In: scalar function result splats.
- Out: vector comparisons and target-specific SIMD intrinsics.

## Deliverables
- Typeck rules for vector binary operators.
- CFG `VectorSplat` or equivalent.
- LLVM elementwise arithmetic emission.
- Reduced fixtures from `scal-to-vec1`, `scal-to-vec2`, and `scal-to-vec3`.

## Acceptance
- Integer vector arithmetic matches scalar lane-by-lane results.
- Floating vector arithmetic matches scalar lane-by-lane results for the reduced fixtures.
- Scalar operands are converted to the vector element type before splatting.

## References
- `docs/gnu-vector-design.md`
- `gcc-torture::execute::scal-to-vec1`
- `gcc-torture::execute::scal-to-vec2`
- `gcc-torture::execute::scal-to-vec3`
