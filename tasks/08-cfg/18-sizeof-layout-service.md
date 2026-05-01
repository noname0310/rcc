# 08-18: Sizeof layout service

> ✓ done — 2026-05-01

**Phase:** 08-cfg    **Depends on:** 08-17    **Milestone:** M6 stabilization

## Goal
Remove hard-coded target layout and silent `sizeof` fallbacks from CFG
lowering. `sizeof` must either query a shared layout service or fail
with a diagnostic before CFG reaches codegen.

## Scope
- In: replace `const_size_of_ty`'s duplicated ABI constants with a
  layout query shared with, or explicitly owned by, codegen layout.
- In: make `sizeof(vla)` multiply by a checked element layout.
- In: avoid `unwrap_or(0)` and avoid returning constant `0` for unknown
  non-VLA types.
- In: tests for scalar, pointer, array, record, nested array, and VLA
  element types.
- Out: final multi-target ABI matrix; one target layout context is
  sufficient if it is centralized.

## Deliverables
- A `LayoutCx`-style API usable by CFG lowering without depending on
  LLVM.
- CFG lowering plumbed with layout context/session target information.
- Regression tests for `sizeof(struct S)`, `sizeof(int[n])`, and
  `sizeof(struct S[n])`.
- A clear error path for incomplete or unsupported layout instead of
  silently materializing `0`.

## Acceptance
- `rg "unwrap_or\\(0\\)" crates/rcc_cfg/src` finds no `sizeof` fallback.
- `sizeof` for a record VLA element does not lower to `n * 0`.
- CFG and LLVM codegen use the same size/alignment answers for the
  supported target.

## References
- C99 §6.5.3.4 `sizeof`.
- `crates/rcc_cfg/src/lower.rs` `lower_sizeof_expr`.
- `crates/rcc_codegen_llvm/src/layout.rs`.
