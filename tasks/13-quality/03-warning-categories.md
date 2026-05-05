> ✓ done — 2026-05-05

# 13-03: Warning categories and common warnings

**Phase:** 13-quality    **Depends on:** 10-09, 13-02    **Milestone:** M7

## Goal
Make warning control a release-quality contract rather than an ad-hoc driver
flag. Define stable warning names, group membership, default severity, and
the point in the pipeline where each warning is emitted.

## Scope
- In:
  - Define warning categories: default, `-Wall`, `-Wextra`, and extension
    warnings.
  - Define aliases for the common warning names implemented by follow-up
    tasks:
    - `-Wunused-variable` (13-03a)
    - `-Wunused-function` (13-03b)
    - `-Wunused-parameter` (13-03c)
    - `-Wimplicit-function-declaration` (13-03d)
    - `-Wsign-compare` (13-03e)
    - `-Wunreachable-code` (13-03f)
  - Make `-Wall` / `-Wextra` group enablement queryable by warning name.
  - Add `-Werror=<name>` / `-Wno-error=<name>` tests for named warnings.
  - Preserve existing GNU-extension warning names and suppression flags.
- Out:
  - Implementing the detector passes themselves; owned by 13-03a..13-03f.
  - `-Wformat`.
  - `-Wconversion`.
  - Flow-sensitive dataflow beyond simple block-local checks.

## Deliverables
- Warning group definitions (Wall, Wextra membership).
- Query APIs for "is this named warning enabled/promoted/suppressed?".
- Tests for warning group membership and named promotion/suppression.
- `docs/warnings.md` describing every warning and group.

## Acceptance
- `-Wall` enables all listed warnings.
- `-Wextra` enables only the documented superset.
- Follow-up detector tasks exist for each listed warning.
- `-Wno-unused-variable` suppresses only that warning once the detector emits
  it.
- `-Werror=<name>` promotes only that warning.
- Warnings include a visible `[-W<name>]` or equivalent note for
  discoverability.

## References
- GCC warning options and their group membership.
