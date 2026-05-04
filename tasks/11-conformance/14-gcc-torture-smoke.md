> ✓ done — 2026-05-04

# 11-14: gcc-torture smoke

**Phase:** 11-conformance    **Depends on:** 01-03    **Milestone:** M4

## Goal
Start running a tiny curated subset of gcc-torture `execute/` tests
(30-50 files) — the simplest ones covering arithmetic, loops, and
pointers. Serves as an early warning for regressions, not a gate.

## Scope
- In: new adapter `GccTortureAdapter::run_subset(&[file])`; maintain
  the subset list under `third_party/testsuites/gcc-torture/smoke-subset.txt`.
- Out: full torture suite (task 07).

## Deliverables
- Subset file + adapter branch.
- manual CI job (respects GPL gate).

## Acceptance
- Nightly CI exposes numbers for the subset.
- Subset picked so ≥ 70 % pass rate is achievable at M4.

## Result
- Added a tracked smoke subset list with 35 `gcc.c-torture/execute` files.
- Added `GccTortureAdapter` discovery and run support behind the existing
  GPL gate; the GPL source checkout remains ignored.
- Added a workflow-dispatch CI job that fetches gcc-torture with
  `--include-gpl`, builds the LLVM-enabled driver, runs the subset, and
  uploads the JSON report.
- Local WSL validation: 35 discovered, 35 passed, 0 failed, pass rate 1.000.
- Larger gcc-torture failures involving compiler gaps such as
  `__builtin_memcpy`/`__builtin_memset` remain outside this smoke subset and
  should become explicit compiler-bug tasks if promoted into a larger gate.

## References
- Plan §10 M4.
