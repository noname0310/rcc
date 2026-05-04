# 00-02: KPI dashboard

**Phase:** 00-overview    **Depends on:** 00-01    **Milestone:** M0.5+

## Goal
Turn the milestone pass-rate targets from plan §10 into a single
machine-checkable document at `docs/conformance.md`. Every task in
phases 11 / 13 references a numeric gate here rather than repeating the
numbers inline, so changing a target is a one-file edit.

## KPI matrix

| Milestone | c-testsuite | chibicc subset                | gcc-torture execute | csmith bounded |
|-----------|------------:|-------------------------------|--------------------:|-------------|
| M1        | parse-only: small corpus boots | —                | —                   | —           |
| M2        | hello-world class             | stages 1–3         | —                   | —           |
| M3        | **≥ 40 %**                    | basic ops / control | —                   | —           |
| M4        | **≥ 70 %**                    | +composite types   | smoke subset begins | —           |
| M5        | ≥ 80 %                        | **preprocessor** tests green | —       | —           |
| M6        | **≥ 95 %**                    | full               | **≥ 60 %**          | —           |
| M7        | stable 95 %+                  | full               | ≥ 70 %              | **no regression** |

Cells the conformance runner is responsible for populating live in
`docs/conformance.md`; this file is the contract.

## Progress source of truth

`rcc_conformance::run_suites` emits `docs/conformance.json`. A tiny
renderer (task [`11-conformance/01-ctestsuite-m3-40pct.md`](../11-conformance/01-ctestsuite-m3-40pct.md)
etc.) writes a markdown table back into `docs/conformance.md`. The
percentage in the `Pass` column is `(pass + xfail) / discovered`.

## Acceptance
- Every conformance task in `11-conformance/` references one row of
  the matrix above by milestone and suite name.
- CI's conformance job fails if any required cell misses its target.

## References
- Plan §10 milestones.
- `crates/rcc_conformance/src/lib.rs` for the data model.
