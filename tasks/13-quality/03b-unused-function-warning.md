> ✓ done — 2026-05-05

# 13-03b: `-Wunused-function`

**Phase:** 13-quality    **Depends on:** 13-03    **Milestone:** M7

## Goal
Warn for `static` functions that are defined but never referenced when
`-Wall` or `-Wunused-function` is enabled.

## Scope
- In:
  - Track internal-linkage function definitions.
  - Track direct function references and calls.
  - Suppress with `-Wno-unused-function`.
- Out:
  - Whole-program analysis across separately compiled translation units.
  - External functions.

## Deliverables
- Detector pass and tests.
- Docs entry in `docs/warnings.md`.

## Acceptance
- `static int helper(void) { return 1; } int main(void) { return 0; }` warns.
- Calling `helper()` suppresses the warning.
- Non-static functions do not warn.
