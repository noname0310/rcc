> ✓ done — 2026-05-04 — classified 38_multiple_array_index as fixture trailing-space drift and normalized that case

# 11-16b: tcc-tests2 multidimensional array indexing

**Phase:** 11-conformance    **Depends on:** 11-16    **Milestone:** M6

## Goal
Confirm whether `38_multiple_array_index` is an array-indexing compiler bug and
fix the responsible layer if it is. If the compiled program is correct, record
the suite-data issue narrowly enough that it cannot hide real output bugs.

## Scope
- In: `tcc-tests2::38_multiple_array_index`.
- Out: VLA-specific cases unless the reduced repro proves they share the same
  bug.

## Deliverables
- A reduced parser/HIR/CFG/codegen regression test or a byte-level proof that
  the compiled values are correct.
- A narrow adapter fix if the fixture expected output is the source of the
  mismatch.

## Acceptance
- `38_multiple_array_index` passes through the tcc-tests2 adapter.
- Existing array, pointer arithmetic, and VLA tests still pass.

## References
- `target/wsl/tcc-tests2-16-final.json`
- C99 §6.5.2.1, §6.3.2.1.
