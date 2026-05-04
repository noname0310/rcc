# 11-15d: gcc-torture VLA layout codegen

> ✓ done — 2026-05-04

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

## Result
- The representative failures were static `sizeof(type)` array bounds
  misclassified as block-scope VLAs during HIR lowering, not true runtime VLA
  codegen failures.
- HIR declarator lowering now constant-folds C99 `sizeof(type-name)` array
  bounds through the shared layout service before deciding whether a block
  array is a VLA.
- Added a regression test proving `char a[sizeof(unsigned)]` lowers as a fixed
  array and has a compile-time layout.
- WSL LLVM probes passed for `gcc-torture::execute::20001228-1`,
  `gcc-torture::execute::memcpy-2`, `gcc-torture::execute::memset-1`, and
  `gcc-torture::execute::strcmp-1`.
- No xfail, skip, or result masking was added.

## References
- `target/wsl/gcc-torture-full-15-final.json`
