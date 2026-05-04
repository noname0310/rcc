# 11a: function.c stage common support

**Phase:** 11-conformance    **Depends on:** 11-11    **Milestone:** M4+

## Goal
Let the stage-isolated chibicc `function.c` run link-time helper functions from
upstream `test/common` without compiling that helper with `rcc`.

## Scope
- In:
  - In `ChibiccAdapter::run_stage_1_to_3`, detect `chibicc::function`.
  - Compile `third_party/testsuites/chibicc/test/common` with host `cc` and
    link it beside the `rcc`-compiled `function.c` object.
  - Keep `arith.c` and `control.c` on the minimal generated `assert` helper
    unless they need more.
  - Add adapter tests proving `function.c` uses the common-helper path and
    other stage fixtures do not.
- Out:
  - Compiling upstream `test/common` with `rcc`.
  - Implementing libc bodies inside `rcc`.

## Deliverables
- Focused conformance adapter branch for the `function.c` stage helper.
- Unit tests for command construction or observable missing-helper behavior.

## Acceptance
- `function.c` no longer fails later with unresolved helper symbols such as
  `true_fn`, `add_all`, `add_float`, or `struct_test4` once compilation
  reaches link.
- `arith.c` and `control.c` stay green in stage-isolated mode.
- No xfail/skip is added for `chibicc::function`.

## References
- chibicc `test/common`.
- `tasks/11-conformance/11-chibicc-function-prereq-triage.md`.
