# 11-02: c-testsuite @ M4 ≥ 70 %

> ✓ done — 2026-05-04

**Phase:** 11-conformance    **Depends on:** 11-01, 06-07, 09-02    **Milestone:** M4

## Goal
Push c-testsuite past 70 % once composite types (struct/union/enum),
aggregate initialisers, pointer arithmetic, string literals, and
globals are in.

## Scope
- In: triage what's still failing after composite support lands;
  close the remaining bugs.
- Out: VLA + variadic (M6).

## Deliverables
- Resolution of xfails added during 11-01.
- New xfails only if justified by M6 feature work.

## Acceptance
- Pass rate ≥ 70 % on CI, stable for 3 runs.

## Completion notes
- Root cause for the largest failure bucket was in the conformance
  harness, not compiler semantics: Windows checkouts materialized
  upstream `.expected` files with CRLF, while Linux executions print LF.
- `CTestSuiteAdapter` now normalizes CRLF to LF for stdout comparison.
- Three consecutive WSL/Linux c-testsuite runs reported identical
  results: `220 cases: 151 pass, 64 fail, 5 xfail, 0 skip; pass_rate=0.709`.
- No new xfail entries were added for this M4 gate.

## References
- Plan §10 M4.
