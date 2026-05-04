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
  - Implement or explicitly defer these common warnings:
  - `-Wunused-variable`: local variable declared but never read.
  - `-Wunused-function`: static function defined but never called.
  - `-Wunused-parameter`: function parameter never used.
  - `-Wimplicit-function-declaration`: calling an undeclared
    function (C99 removed implicit int, but this is common).
  - `-Wsign-compare`: comparison between signed and unsigned.
  - `-Wunreachable-code`: code after `return`/`break`/`continue`/
    `goto` in the same block.
  - Preserve existing GNU-extension warning names and suppression flags.
- Out:
  - `-Wformat`.
  - `-Wconversion`.
  - Flow-sensitive dataflow beyond simple block-local checks.

## Deliverables
- Warning group definitions (Wall, Wextra membership).
- Detection passes for each warning.
- Tests for each warning with `-Wall`, `-Wextra`, and `-Wno-<name>`.
- `docs/warnings.md` describing every warning and group.

## Acceptance
- `-Wall` enables all listed warnings.
- `-Wextra` enables only the documented superset.
- Each warning fires on the expected pattern.
- `-Wno-unused-variable` suppresses only that warning.
- `-Werror=<name>` promotes only that warning.
- Warnings include a visible `[-W<name>]` or equivalent note for
  discoverability.

## References
- GCC warning options and their group membership.
