# 11-09: llvm-test-suite SingleSource

**Phase:** 11-conformance    **Depends on:** 01-05    **Milestone:** M7

## Goal
Run a curated subset of `llvm-test-suite/SingleSource/UnitTests/`.
Many tests have reference output files; adapter compares them.

## Scope
- In: `LlvmTestSuiteAdapter` implementation; cherry-pick a subset
  (ARL / UnitTests / Regression) to keep runtime under 30 min.
- Out: MultiSource / benchmarks (future).

## Deliverables
- Adapter + nightly job.

## Acceptance
- Subset pass rate reported; no unexplained regressions for 3
  consecutive nights.

## References
- Plan §10 M7.
