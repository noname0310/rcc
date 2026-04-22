# 11-02: c-testsuite @ M4 ≥ 70 %

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

## References
- Plan §10 M4.
