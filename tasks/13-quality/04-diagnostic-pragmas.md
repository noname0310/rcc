# 13-04: Diagnostic pragmas

**Phase:** 13-quality    **Depends on:** 13-03    **Milestone:** M7

## Goal
Implement `#pragma GCC diagnostic push/pop/ignored/warning/error` so source
files can locally control the warning policy defined in task 13-03.

## Scope
- In:
  - Parse `#pragma GCC diagnostic push`, `pop`, `ignored "-Wname"`,
    `warning "-Wname"`, and `error "-Wname"`.
  - Maintain a severity override stack in the diagnostic policy layer.
  - Apply the policy to warnings emitted after the pragma, including warnings
    from downstream parser/HIR/typeck stages.
  - Emit a warning for malformed diagnostic pragmas instead of silently
    ignoring them.
- Out:
  - `#pragma clang diagnostic` alias.
  - Cross-translation-unit policy persistence.

## Deliverables
- Pragma handler for `GCC diagnostic` directives.
- Override stack or equivalent scoped policy in diagnostics/session state.
- Tests: push/ignored/pop restores original severity.

## Acceptance
- `#pragma GCC diagnostic ignored "-Wunused-variable"` suppresses
  the warning within scope.
- `#pragma GCC diagnostic push` / `pop` correctly saves and
  restores state.
- `#pragma GCC diagnostic error "-Wunused-variable"` promotes the
  warning to an error.
- Malformed diagnostic pragmas do not panic and produce one stable diagnostic.
- `cargo test -p rcc_preprocess -p rcc_driver --all-targets` passes.

## References
- GCC diagnostic pragmas documentation.
