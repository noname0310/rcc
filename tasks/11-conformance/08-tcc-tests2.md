# 11-08: tcc-tests2

**Phase:** 11-conformance    **Depends on:** 01-04    **Milestone:** M6

## Goal
Run TinyCC's `tests/tests2/` via a dedicated adapter. Each file is
paired with an expected reference output; pass rate populates the
KPI row.

## Scope
- In: `TccTests2Adapter` implementation (skeleton exists); target a
  modest pass rate (≥ 40 % at M6) since tcc tests exercise weird
  edges.
- Out: tcc's "runtime" tests that need its libtcc headers.

## Deliverables
- Adapter + nightly CI job.
- Report row.

## Acceptance
- Pass-rate number printed; xfails for known-extension tests.

## References
- Plan §10.
