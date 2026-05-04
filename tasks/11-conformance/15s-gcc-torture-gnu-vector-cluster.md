# 11-15s: gcc-torture GNU vector cluster

**Phase:** 11-conformance    **Depends on:** 11-15k    **Milestone:** M6

## Goal
Define and implement the first coherent slice of GNU vector extension support
needed by gcc-torture.

## Scope
- In: `20050316-1`, `20050316-2`, `20050316-3`, `20050604-1`, `pr92618`,
  `scal-to-vec1`, `scal-to-vec2`, `scal-to-vec3`, `simd-4`, `simd-6`.
- Out: target-specific SIMD intrinsics.

## Deliverables
- A mini-design for `vector_size` types in AST/HIR/typeck/CFG/LLVM.
- Reduced fixtures for vector literals, scalar-vector casts, vector arithmetic,
  vector loads/stores, and vector ABI.
- Follow-up implementation tasks if this must span multiple commits.

## Acceptance
- No vector case is treated as a generic compiler bug without a vector-specific
  task.
- At least one vector runtime case passes or the implementation task split is
  complete.

## References
- `docs/gcc-torture-signal-clusters.md`
