# 13-08: Fuzz crash artifacts become regression tests

> ✓ done — 2026-05-05

**Phase:** 13-quality    **Depends on:** 12-05, 13-07    **Milestone:** M7

## Goal
Close the loop between GitHub Actions fuzz failures and permanent tests. A
fuzz crash is not "handled" until it is reproduced, minimized when useful,
fixed, and stored as a regression seed or unit test.

## Scope
- In:
  - Document how to fetch artifacts with `gh run download`.
  - Add a local helper script or `xtask fuzz-regression` command that copies a
    crash into the right corpus directory and prints the reproduce command.
  - Add the recent preprocessor recursive-include crash as either a corpus
    seed or point to the unit test that now covers it.
  - Ensure lexer, preprocess, and parse fuzz workflows upload artifacts on
    failure and success.
- Out:
  - 24h scheduled fuzzing.
  - Team-channel notification assumptions.

## Deliverables
- `docs/fuzzing.md` with reproduce/minimize/promote workflow.
- Script or `xtask` helper for crash promotion.
- Regression seed or explicit unit-test link for every known crash artifact.

## Acceptance
- A developer can run the command from a failed Actions log locally without
  guessing paths.
- `fuzz/corpus/*` contains only intentional seeds, not raw unreviewed crash
  spam.
- The preprocess recursive-include crash does not recur on the current head.

## References
- `.github/workflows/fuzz-*-30m.yml`.
- `fuzz/fuzz_targets/`.
