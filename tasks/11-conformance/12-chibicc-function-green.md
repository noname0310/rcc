> ✓ done — 2026-05-04

# 11-12: chibicc function.c green

**Phase:** 11-conformance    **Depends on:** 11-10, 11a, 11b, 11c, 11d    **Milestone:** M6

## Goal
Make the stage-isolated chibicc `function.c` TU pass end-to-end.

## Scope
- In:
  - Implement the remaining blockers identified by `11-10` when they belong
    in this phase.
  - Verify function calls, recursion, static locals, varargs, function
    pointers, `__func__`/`__FUNCTION__`, float/double calls, and aggregate
    argument/return behavior needed by the fixture.
- Out:
  - Full chibicc suite green.

## Deliverables
- Green conformance report for `chibicc::function`.
- Focused regression tests for every compiler/runtime bug fixed.

## Acceptance
- `rcc_conformance_run --suite chibicc --mode <stage-1-3>` reports
  `chibicc::function` as `pass`.
- All three stage-isolated TUs (`arith`, `control`, `function`) are green in
  the same report.

## Notes (agent)
- Added explicit `-fgnu89-inline`/`-fgnu-inline`/`-fchibicc-inline` compatibility
  wiring so chibicc's plain `inline` definition is emitted for stage runs
  without changing strict C99 defaults.
- Fixed SysV aggregate ABI unit selection for mixed integer/SSE eightbytes.
  The `Ty5 { int; float; double; }` chibicc fixture was previously classified
  as `[i32, double]`, losing the `float` member when crossing the host-compiled
  support-object boundary; it now lowers as `[i64, double]`.
- WSL stage report: `arith`, `control`, and `function` all pass.

## References
- chibicc `test/function.c`.
