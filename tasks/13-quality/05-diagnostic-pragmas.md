# 13-05: Diagnostic pragmas

**Phase:** 13-quality    **Depends on:** —    **Milestone:** M7

## Goal
Implement `#pragma GCC diagnostic push/pop/ignored/warning/error`
to allow source-level control of diagnostic severity. Extend the
diagnostic handler with a stack of severity overrides.

## Scope
- In: parse `#pragma GCC diagnostic push`, `pop`,
  `ignored "-Wname"`, `warning "-Wname"`, `error "-Wname"`.
  Maintain a stack in the diagnostic handler: `push` saves current
  overrides, `pop` restores. `ignored/warning/error` sets the
  severity for the named warning in the current scope.
- Out: `#pragma clang diagnostic` (alias — trivial follow-up).

## Deliverables
- Pragma handler for `GCC diagnostic` directives.
- Override stack in `Handler`.
- Tests: push/ignored/pop restores original severity.

## Acceptance
- `#pragma GCC diagnostic ignored "-Wunused-variable"` suppresses
  the warning within scope.
- `#pragma GCC diagnostic push` / `pop` correctly saves and
  restores state.
- `#pragma GCC diagnostic error "-Wunused-variable"` promotes the
  warning to an error.

## References
- GCC diagnostic pragmas documentation.
