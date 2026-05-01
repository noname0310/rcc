# 08-22: Source pipeline edge fixtures

**Phase:** 08-cfg    **Depends on:** 08-21    **Milestone:** M3 stabilization

## Goal
Add end-to-end source fixtures for the edge cases that CFG unit tests
missed because they were either hand-built in upstream crate tests or
were intentionally avoided by the initial fixture table.

## Scope
- In: source-to-CFG tests for `for (int i = 0; ...; ++i)`, postfix
  increment returns, goto out of nested scope, goto around VLA scope,
  record-sized `sizeof`, record-element VLA `sizeof`, and complex
  conversion expressions.
- In: every fixture should go through preprocess -> parse -> HIR ->
  typeck -> CFG, not a manually constructed HIR body.
- In: snapshots only for cases where textual MIR adds useful review
  signal; otherwise use structural assertions.
- Out: phase 09 LLVM execution tests.

## Deliverables
- Extend `crates/rcc_cfg/tests/cfg.rs` with edge-case fixtures and
  targeted assertions.
- Add snapshots for at least the goto lifetime and complex conversion
  cases.
- Add a comment block listing which former review finding each fixture
  guards.

## Acceptance
- `cargo test -p rcc_cfg edge` passes.
- The fixture table contains at least one source-level test for every
  08-16 through 08-21 stabilization task.
- No fixture relies on undefined or unspecified C behavior for its pass
  condition.

## References
- `crates/rcc_cfg/tests/cfg.rs`.
- 08-cfg stabilization tasks 16-21.
