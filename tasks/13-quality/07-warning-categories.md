# 13-07: Warning categories and common warnings

**Phase:** 13-quality    **Depends on:** 10-09    **Milestone:** M7

## Goal
Define warning categories (`-Wall`, `-Wextra`) and implement the
most common individual warnings: unused variable/function/parameter,
implicit function declaration, signedness comparison mismatch, and
unreachable code after return.

## Scope
- In: define which warning codes belong to `-Wall` vs `-Wextra`.
  Implement individual warnings:
  - `-Wunused-variable`: local variable declared but never read.
  - `-Wunused-function`: static function defined but never called.
  - `-Wunused-parameter`: function parameter never used.
  - `-Wimplicit-function-declaration`: calling an undeclared
    function (C99 removed implicit int, but this is common).
  - `-Wsign-compare`: comparison between signed and unsigned.
  - `-Wunreachable-code`: code after `return`/`break`/`continue`/
    `goto` in the same block.
- Out: `-Wformat` (printf format checking), `-Wconversion`.

## Deliverables
- Warning group definitions (Wall, Wextra membership).
- Detection passes for each warning.
- Tests for each warning with `-Wall` and `-Wno-<name>`.

## Acceptance
- `-Wall` enables all listed warnings.
- Each warning fires on the expected pattern.
- `-Wno-unused-variable` suppresses only that warning.
- Warnings include a `-W<name>` note for discoverability.

## References
- GCC warning options and their group membership.
