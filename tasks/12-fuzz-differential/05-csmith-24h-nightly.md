# 12-05: csmith 24 h nightly

**Phase:** 12-fuzz-differential    **Depends on:** 12-04    **Milestone:** M7

## Goal
Run the csmith differential harness for 24 h nightly. Track
disagreement rate over time; any non-zero rate opens an issue.

## Scope
- In: nightly GitHub Action; archive disagreement repro cases
  under `reports/csmith/<date>/<id>/`.
- Out: auto-reducing the repro (future; consider `creduce`).

## Deliverables
- nightly workflow.
- Reports bucket.

## Acceptance
- 3 consecutive nights with 0 disagreements = M7 KPI green.

## References
- Plan §10 M7.
