# 08-15: CFG lowering unit tests

**Phase:** 08-cfg    **Depends on:** 08-05 .. 08-14    **Milestone:** M3

## Goal
Build a `tests/cfg.rs` that lowers small HIR snippets and asserts
CFG invariants (every block terminator present, every local covered by
StorageLive/Dead, etc.) plus snapshots the dumps.

## Scope
- In: `lower_snippet(src) -> Body`; programmatic invariant checks;
  `insta` snapshots.
- Out: codegen (phase 09).

## Deliverables
- `tests/cfg.rs` with ≥ 25 fixtures.

## Acceptance
- `cargo test -p rcc_cfg`: green; `cargo llvm-cov`: ≥ 75 %.

## References
- Plan §8.2 "rcc_cfg: 로워링 스냅샷".
