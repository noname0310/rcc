# 11-15d: gcc-torture VLA layout codegen

**Phase:** 11-conformance    **Depends on:** 11-15    **Milestone:** M6

## Goal
Remove codegen failures caused by requesting compile-time layout for runtime
VLA-dependent types.

## Scope
- In: failures containing `has no compile-time layout: VLA size is runtime` or
  `cannot compute layout for sizeof operand` when the construct is valid C99.
- Out: invalid VLA constructs and non-C99 extensions.

## Deliverables
- Reduced HIR/CFG/codegen test that exercises a currently failing VLA case.
- Runtime-size path for the affected lowering/codegen operation, or an earlier
  semantic diagnostic when the source is invalid.
- gcc-torture rerun summary for the targeted cluster.

## Acceptance
- Representative cases `20001228-1`, `memcpy-2`, `memset-1`, and `strcmp-1`
  no longer fail with internal layout errors when they are valid C99.
- Internal errors are replaced by either correct codegen or user-facing
  diagnostics.

## References
- `target/wsl/gcc-torture-full-15-final.json`
