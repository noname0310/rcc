> ✓ done — 2026-05-05 — bounded csmith differential workflow

# 12-05: csmith bounded differential run

**Phase:** 12-fuzz-differential    **Depends on:** 12-04    **Milestone:** M7

## Goal
Run the csmith differential harness with a bounded manual or
path-filtered budget. Track disagreement rate over time; any non-zero
rate opens a task.

## Scope
- In: GitHub Action; archive disagreement repro cases
  under `reports/csmith/<date>/<id>/`.
- Out: auto-reducing the repro (future; consider `creduce`).

## Deliverables
- bounded workflow.
- Reports bucket.

## Acceptance
- 3 consecutive 30 minute runs with 0 disagreements = M7 KPI green.

## Completion notes
- Added `.github/workflows/csmith-bounded.yml`.
- The workflow fetches and builds csmith, builds LLVM-enabled `rcc`, then
  runs `rcc_csmith_diff` for a default 1800 second budget.
- It is path-filtered to compiler/codegen/conformance changes and manually
  dispatchable with `max_duration_secs`, `iterations`, and
  `max_source_bytes` inputs.
- Reports and preserved disagreement cases are uploaded from
  `reports/csmith/<github-run-id>/`.

## References
- Plan §10 M7.
