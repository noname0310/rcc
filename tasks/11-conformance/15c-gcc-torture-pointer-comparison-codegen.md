# 11-15c: gcc-torture pointer comparison codegen

> ✓ done — 2026-05-04

**Phase:** 11-conformance    **Depends on:** 11-15    **Milestone:** M6

## Goal
Fix internal LLVM codegen errors for pointer equality/inequality comparisons.

## Scope
- In: failures containing `equality operands have incompatible LLVM types`.
- Out: unrelated pointer arithmetic or provenance semantics.

## Deliverables
- A reduced driver or codegen test reproducing one gcc-torture failure.
- Codegen/typeck normalization so pointer comparisons use compatible LLVM
  values before `icmp`.
- Full gcc-torture rerun summary for the targeted cluster.

## Acceptance
- Representative cases `20000910-2`, `20010711-1`, `20020129-1`, and
  `20050826-2` no longer fail with internal codegen errors.
- No internal compiler error remains for compatible object/function pointer
  equality comparisons.

## Result
- LLVM codegen now normalizes pointer/integer equality and inequality through
  an `intptr` comparison, covering C null pointer constants emitted as integer
  constants.
- Added an LLVM feature regression test for pointer-null equality lowering.
- WSL probes passed for `gcc-torture::execute::20000910-2`,
  `gcc-torture::execute::20010711-1`, `gcc-torture::execute::20020129-1`, and
  `gcc-torture::execute::20050826-2`.
- No xfail, skip, or conformance-result masking was added.

## References
- `target/wsl/gcc-torture-full-15-final.json`
