# 11-16b: tcc-tests2 multidimensional array indexing

**Phase:** 11-conformance    **Depends on:** 11-16    **Milestone:** M6

## Goal
Fix incorrect output for nested array indexing and row-major address
calculation.

## Scope
- In: `tcc-tests2::38_multiple_array_index`.
- Out: VLA-specific cases unless the reduced repro proves they share the same
  bug.

## Deliverables
- A reduced parser/HIR/CFG/codegen regression test for `a[i][j]` on nested
  arrays.
- A fix in the layer that miscomputes the element address or decay.

## Acceptance
- `38_multiple_array_index` passes through the tcc-tests2 adapter.
- Existing array, pointer arithmetic, and VLA tests still pass.

## References
- `target/wsl/tcc-tests2-16-final.json`
- C99 §6.5.2.1, §6.3.2.1.
