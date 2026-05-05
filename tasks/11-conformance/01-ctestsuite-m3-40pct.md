# 11-01: c-testsuite @ M3 ≥ 40 %

> ✓ done — 2026-05-04

**Phase:** 11-conformance    **Depends on:** 08-* completion    **Milestone:** M3

## Goal
Reach a 40 %+ pass rate on c-testsuite by the time M3 lands (CFG +
codegen MVP). That's roughly integer sources, simple function calls,
basic control flow, and pointer dereferences.

## Scope
- In: identify the c-testsuite files that currently fail; triage into
  (a) real bugs to fix, (b) missing-feature xfails, (c) adapter gaps.
- Out: struct / union tests (deferred to M4 task 02).

## Deliverables
- xfail entries pointing at future tasks.
- Bug-fix commits/tasks for anything unblockable.
- `docs/conformance.md` row: c-testsuite column shows ≥ 40 %.

## Acceptance
- `cargo run --release --package rcc_conformance -- --suite c-testsuite`
  prints `pass_rate >= 0.40` on three consecutive CI runs.

## Completion notes
- WSL/Linux run command:
  `cargo run -p rcc_conformance --bin rcc_conformance_run --release -- --rcc /tmp/rcc-wsl-target/release/rcc --suite c-testsuite`.
- Three consecutive local runs reported identical results:
  `220 cases: 116 pass, 99 fail, 5 xfail, 0 skip; pass_rate=0.550`.
- No new xfail entries were added for this M3 gate; the existing 5
  c-testsuite xfails remain explicit C99/out-of-scope or known follow-up
  items.

## References
- Plan §10 M3.
- Task 00-02 KPI matrix row for M3.
