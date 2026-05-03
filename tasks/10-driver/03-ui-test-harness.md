> ✓ done — 2026-05-04

# 10-03: UI test harness

**Phase:** 10-driver    **Depends on:** 02-01    **Milestone:** M2

## Goal
Run every `.c` under `crates/rcc_driver/tests/ui/**/` through the
driver, compare stderr byte-for-byte against the sibling `.stderr`
fixture. Supports `UPDATE_EXPECT=1` to re-generate.

## Scope
- In: test runner binary `tests/ui.rs` using `trybuild`-style discovery;
  normalise paths (strip absolute prefixes).
- Out: running the linked binary (task 05).

## Deliverables
- Harness + directory layout in `tests/ui/parse/`, `tests/ui/typeck/`.
- Regression: at least 5 fixtures committed.

## Acceptance
- `cargo test -p rcc_driver --test ui`: all snapshots identical.
- `UPDATE_EXPECT=1 cargo test --test ui`: rewrites `.stderr`.

## References
- rustc `compiletest`; `trybuild`.
