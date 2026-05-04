# 11-15c: gcc-torture pointer comparison codegen

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

## References
- `target/wsl/gcc-torture-full-15-final.json`
