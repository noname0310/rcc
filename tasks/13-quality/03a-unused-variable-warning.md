# 13-03a: `-Wunused-variable`

**Phase:** 13-quality    **Depends on:** 13-03    **Milestone:** M7

## Goal
Warn for local variables that are declared but never read when `-Wall` or
`-Wunused-variable` is enabled.

## Scope
- In:
  - Track local declarations and local-reference reads in HIR or CFG.
  - Treat writes-only as unused unless the variable is volatile.
  - Suppress the warning with `-Wno-unused-variable`.
  - Promote with `-Werror=unused-variable`.
- Out:
  - Flow-sensitive liveness.
  - Warnings for globals or parameters.

## Deliverables
- Detector pass and driver/typeck tests.
- Docs entry in `docs/warnings.md`.

## Acceptance
- `int f(void) { int x; return 0; }` warns under `-Wall`.
- A read of `x` suppresses the warning.
- `volatile int x;` does not warn.
- `-Wno-unused-variable` suppresses only this warning.
