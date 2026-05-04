# 11-15s6: GNU vector ABI for parameters and returns

> ✓ done — 2026-05-04

**Phase:** 11-conformance    **Depends on:** 11-15s5    **Milestone:** M6

## Goal
Pass and return fixed-size vectors through the SysV ABI without scalar or
aggregate fallback corruption.

## Scope
- In: 32-bit, 64-bit, and 128-bit fixed vectors used by gcc-torture.
- In: direct vector params and returns.
- Out: platform-specific non-SysV vector ABI differences.

## Deliverables
- ABI classification for `Ty::Vector`.
- LLVM function type lowering for vector params/returns.
- Runtime fixtures for vector function calls and returns.

## Acceptance
- Vector arguments arrive with correct lane bytes.
- Vector returns preserve lane bytes through callers.
- The 20050316 reduced call/return cases pass.

## References
- `docs/gnu-vector-design.md`
- `gcc-torture::execute::20050316-1`
- `gcc-torture::execute::20050316-2`
- `gcc-torture::execute::20050316-3`
