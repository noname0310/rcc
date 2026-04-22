# 11-05: chibicc preprocessor tests

**Phase:** 11-conformance    **Depends on:** 04-18    **Milestone:** M5

## Goal
All preprocessor-focused chibicc fixtures (`macro.c`, `include.c`,
`typedef.c`) pass. Landmark for M5 completion.

## Scope
- In: run via `ChibiccAdapter::run_preprocess_only`; fix anything
  still failing.
- Out: full M5 integration (other tests still running).

## Deliverables
- Green KPI cell.

## Acceptance
- `rcc --emit=pp chibicc-fixture.c` matches `cc -E` on each file.

## References
- Plan §10 M5.
