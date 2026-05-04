# 11-08: chibicc arith.c green

**Phase:** 11-conformance    **Depends on:** 11-05, 11-06, 11-07    **Milestone:** M2+

## Goal
Make the stage-isolated chibicc `arith.c` TU pass end-to-end.

## Scope
- In:
  - Run only `chibicc::arith` through the stage-isolated mode.
  - Fix remaining arithmetic, pointer arithmetic, compound assignment,
    pre/post inc-dec, conditional-expression, or `sizeof` regressions found by
    this TU.
  - Keep every failure visible until fixed; do not xfail individual assertions.
- Out:
  - `control.c` and `function.c`.

## Deliverables
- Green conformance report for `chibicc::arith`.
- Targeted regression tests for every compiler bug fixed while making it pass.
- Dashboard/KPI note recording the TU as green.

## Acceptance
- `rcc_conformance_run --suite chibicc --mode <stage-1-3>` reports
  `chibicc::arith` as `pass`.
- No new c-testsuite regression.

## References
- chibicc `test/arith.c`.
