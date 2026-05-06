> ✓ done — 2026-05-04

# 11-15a: gcc-torture parser declaration forms

**Phase:** 11-conformance    **Depends on:** 11-15    **Milestone:** M6

## Goal
Turn the largest remaining gcc-torture parser failure cluster into concrete
parser fixes instead of treating the 60% gate as sufficient.

## Scope
- In: failures reported as `expected ';' after declaration`, `expected ')' to
  close parameter list`, and closely related declaration/declarator recovery
  cases in `gcc.c-torture/execute`.
- Out: constructs that are not required for the current ISO C release gates; if
  a failing case is outside the release target, document it and keep it out of
  strict gates.

## Deliverables
- Minimal parser unit or UI tests extracted from representative failing cases.
- Parser fixes or explicit compatibility flags for GNU-only declaration syntax.
- Updated gcc-torture full report showing the cluster reduced.

## Acceptance
- Representative cases such as `20000822-1`, `20001024-1`, `20001121-1`,
  and `20020108-1` no longer fail with parser shape errors unless classified
  as out-of-scope extension syntax.
- No xfail entries are added for ordinary ISO C parser bugs.

## Result
- Added parser keyword aliases for GNU reserved spellings `__restrict`,
  `__restrict__`, `__inline`, and `__inline__`.
- Added `-fgnu89-inline` to the gcc-torture full adapter path so inline
  declarations use the intended compatibility semantics.
- Representative outcomes:
  - `20001024-1`: parser failure fixed, now passes.
  - `20001121-1`: parser failure fixed; link failure fixed by passing
    `-fgnu89-inline`, now passes.
  - `20000822-1`: classified out-of-scope for C99 because it requires GNU
    nested functions/trampolines.
  - `20020108-1`: classified as a C99 preprocessor rescan bug, split into
    task 11-15f instead of being hidden here.
- Full gcc-torture WSL rerun improved from 1104/1650 to 1126/1650 passing.
  Parser-shape clusters reduced: `expected ';' after declaration` 87 -> 77,
  `expected ')' to close parameter list` 15 -> 9.
- No xfail entries were added.

## References
- `target/wsl/gcc-torture-full-15-final.json`
