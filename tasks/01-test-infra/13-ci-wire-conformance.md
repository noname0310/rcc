# 01-13: CI: wire conformance into PR gates

**Phase:** 01-test-infra    **Depends on:** 01-10 .. 01-12    **Milestone:** M0.5

## Goal
Turn the conformance binary into a blocking CI check. The current
milestone level (from `docs/milestone.txt`, a single-line file) picks
which KPI cells are required; failures block merge.

## Scope
- In: `.github/workflows/ci.yml` additions: new `conformance` job
  that checks out, fetches **permissive** suites, builds `rcc`,
  runs the binary, verifies thresholds.
- Out: nightly GCC torture / csmith job (phase 12).

## Deliverables
- `conformance` job uploads `docs/conformance.json` as an artifact.
- `scripts/ci/check_kpi.py` (or similar) reads the JSON and the KPI
  matrix, exits non-zero on violation.
- `docs/milestone.txt` with value `M0.5`.

## Acceptance
- CI green on main after this task lands (KPI at M0.5 is trivially
  satisfied: no suite is required to pass anything yet).
- Intentionally-failing PR demo: edit `docs/milestone.txt` to `M3`
  on a branch and verify CI's conformance job fails because nothing
  actually passes yet.

## References
- `.github/workflows/ci.yml` current `conformance` job stub.
- Task 00-02 KPI matrix.
