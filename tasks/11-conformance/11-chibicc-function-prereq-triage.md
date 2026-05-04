# 11-11: chibicc function.c prerequisite triage

**Phase:** 11-conformance    **Depends on:** 11-08    **Milestone:** M4+

## Goal
Turn chibicc `function.c` from an opaque large-fixture failure into a tracked
checklist of concrete frontend, ABI, runtime, and extension blockers.

## Scope
- In:
  - Run `function.c` in the stage-isolated mode and classify each failure by
    owner crate.
  - Separate C99-required compiler bugs from GNU extension blockers and
    runtime/header gaps.
  - Add or update follow-up tasks in the owning phase when the issue is not
    appropriate for phase 11.
- Out:
  - Making `function.c` green.

## Deliverables
- A failure matrix in this task file or a linked conformance note.
- New focused tasks for any blocker not already tracked.
- No xfail entries that hide the selected TU.

## Acceptance
- Every current `function.c` failure maps to an owner and next task.
- C99-required compiler bugs are not treated as pass-rate noise.
- The next actionable implementation task is unambiguous.

## References
- chibicc `test/function.c`.
- `tasks/15-builtin-rt/`
- ABI-related tasks in `09-codegen-llvm/` and `10-driver/`.
