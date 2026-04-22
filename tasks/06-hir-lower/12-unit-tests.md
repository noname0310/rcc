# 06-12: HIR lowering unit tests

**Phase:** 06-hir-lower    **Depends on:** 06-01 .. 06-11    **Milestone:** M2

## Goal
A single `tests/hir_lower.rs` that walks every feature added in this
phase: declarator table, resolution edges, composite lowering,
initializers.

## Scope
- In: table of `(source, assertion: Box<dyn Fn(&HirCrate)>)`;
  `Session::for_test`.
- Out: type-checked properties (phase 07).

## Deliverables
- `tests/hir_lower.rs` with ≥ 30 rows.
- Helper `lower_snippet(src: &str) -> (HirCrate, TyCtxt)`.

## Acceptance
- `cargo test -p rcc_hir_lower`: green; coverage ≥ 75 %.
- Declarator table: every C99 §6.7.5 example asserted.

## References
- Plan §8.2 "rcc_hir_lower: declarator 트리 → Ty 조립 표".
