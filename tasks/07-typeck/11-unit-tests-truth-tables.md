> ✓ done — 2026-04-27

# 07-11: Truth tables + diagnostic fixtures

**Phase:** 07-typeck    **Depends on:** 07-01 .. 07-10    **Milestone:** M3

## Goal
A central `tests/typeck.rs` with:
- Usual arithmetic conversion table (13×13).
- Integer promotion table.
- Assignability matrix for scalar / pointer / struct.
- UI fixtures for each E0080..E0084.

## Scope
- In: combine `Session::for_test` + assertion helpers; UI fixtures
  live in `tests/ui/typeck/`.
- Out: type-based optimisation checks (M7).

## Deliverables
- `tests/typeck.rs` with ≥ 50 assertions.
- ≥ 10 UI fixtures.

## Acceptance
- `cargo test -p rcc_typeck`: green; `cargo llvm-cov`: ≥ 80 %.

## References
- Plan §8.2 "rcc_typeck: §6.3 usual arithmetic conversion 진리표".
