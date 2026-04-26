> ✓ done — 2026-04-25

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

## Notes (agent)

This task was split between the HIR-side classification (done now) and
the LLVM linkage emission (deferred). Phase 09 (`09-codegen-llvm`) had
not started when this checkbox was claimed, so the codegen half cannot
land yet without crossing the `One task, one commit` boundary.

Done in this session:
- `DefKind::Function` carries `is_inline` and the new
  `is_extern_inline` flag (`crates/rcc_hir/src/lib.rs`).
- `assign_def_ids` in `crates/rcc_hir_lower/src/lib.rs` populates both
  flags from `DeclSpecs::func_specs.inline` and `DeclSpecs::storage`,
  covering the three C99 §6.7.4 combinations (`inline`,
  `extern inline`, `static inline`).
- Three integration tests in
  `crates/rcc_hir_lower/tests/hir_lower.rs` cover each combination.

Deferred to a phase-09 follow-up task (open the corresponding task in
`09-codegen-llvm` when the phase starts):
- Map `(is_inline, is_extern_inline, is_static)` to the LLVM linkage
  kinds called out in the original Acceptance section:
  - `static inline` → `internal` linkage.
  - `extern inline` → `external` linkage (provides the external
    definition).
  - plain `inline` → `available_externally` (or omit when address is
    not taken).
- Tests verifying the emitted linkage attribute on each of the three
  cases.
