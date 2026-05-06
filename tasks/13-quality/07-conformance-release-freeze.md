# 13-07: Conformance release freeze

> ✓ done — 2026-05-05

**Phase:** 13-quality    **Depends on:** 11-19, 13-06    **Milestone:** M7

## Goal
Freeze the release dashboard so users can tell exactly what the compiler
supports. The dashboard must not hide compiler bugs behind aggregate pass
rates.

## Scope
- In:
  - Regenerate `docs/conformance.json` and `docs/conformance.md` from the
    same command sequence documented in `docs/conformance.md`.
  - Confirm required suites have zero non-xfailed failures.
  - Review every xfail entry and classify it as outside-release target,
    implementation gap, external-suite drift, or platform/runtime limitation.
  - Add concrete follow-up tasks for any implementation gap that is not fixed
    before release.
- Out:
  - Making full chibicc exploratory compile mode a release gate.
  - Deleting xfails just to raise the percentage.

## Deliverables
- Refreshed conformance JSON/markdown.
- `docs/release-conformance-policy.md`.
- Follow-up task files for any real compiler bugs that remain out of scope.

## Acceptance
- Required dashboard rows show `Fail = 0`.
- Every `XFail` has a reason that is specific enough for a future task.
- CI's conformance job runs the same required subset described in the docs.

## References
- `docs/conformance.md`.
- `scripts/ci/check_kpi.py`.
