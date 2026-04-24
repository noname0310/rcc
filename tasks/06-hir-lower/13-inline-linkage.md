# 06-13: `inline` function linkage

**Phase:** 06-hir-lower    **Depends on:** 06-06    **Milestone:** M5

## Goal
Implement C99 `inline` function linkage semantics (C99 §6.7.4).
Track `is_inline` and `is_extern_inline` on `DefKind::Function` in
the HIR and emit the correct LLVM linkage for each combination of
`inline`, `extern`, and `static`.

## Scope
- In: three cases per C99 §6.7.4:
  - `inline` (no storage class) → inline definition, no external
    definition provided; emit with `available_externally` or
    internal linkage (no symbol emitted unless address taken).
  - `extern inline` → provides the external definition; emit with
    external linkage.
  - `static inline` → internal linkage, always emitted.
  Track `is_inline` flag in `DefKind::Function`. Propagate to
  codegen for LLVM linkage selection.
- Out: link-time optimisation hints, `__attribute__((always_inline))`.

## Deliverables
- `is_inline` / `is_extern_inline` fields on `DefKind::Function`.
- HIR lowering: detect `inline` specifier from declaration specifiers.
- Codegen: map inline + storage class to LLVM linkage.
- Tests: verify linkage for all three cases.

## Acceptance
- `static inline int f(void) { return 1; }` emits a function with
  `internal` linkage.
- `extern inline int g(void) { return 2; }` emits with `external`
  linkage.
- Plain `inline int h(void) { return 3; }` emits with
  `available_externally` linkage (or is omitted if address not taken).

## References
- C99 §6.7.4 — Function specifiers.
- LLVM linkage types documentation.
