# 11-10: chibicc control.c green

> ✓ done — 2026-05-04

**Phase:** 11-conformance    **Depends on:** 11-08, 11-09    **Milestone:** M3+

## Goal
Make the stage-isolated chibicc `control.c` TU pass end-to-end.

## Scope
- In:
  - Run only `chibicc::control` through the stage-isolated mode.
  - Fix remaining statement-expression, branch, loop, switch, goto,
    break/continue, float-condition, and computed-goto bugs found by this TU.
  - Add focused regression tests for every bug.
- Out:
  - `function.c` ABI/runtime work.

## Deliverables
- Green conformance report for `chibicc::control`.
- Regression fixtures for any control-flow lowering/codegen fix.

## Acceptance
- `rcc_conformance_run --suite chibicc --mode <stage-1-3>` reports
  `chibicc::control` as `pass`.
- Existing c-testsuite control-flow cases remain green.

## References
- chibicc `test/control.c`.
