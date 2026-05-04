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
- nightly-only CI job (respects GPL gate).

## Acceptance
- Nightly CI exposes numbers for the subset.
- Subset picked so ≥ 70 % pass rate is achievable at M4.

## References
- Plan §10 M4.
