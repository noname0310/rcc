# 11-15f: gcc-torture deferred macro rescan

> ✓ done — 2026-05-04

**Phase:** 11-conformance    **Depends on:** 11-15a    **Milestone:** M6

## Goal
Fix C99 macro rescanning for object-like macros whose replacement list names a
function-like macro defined later.

## Scope
- In: cases where an object-like macro body such as `REPEAT_8` contains
  `REPEAT_FN(0)` tokens and `REPEAT_FN` should expand when `REPEAT_8` is used.
- Out: GNU-only macro syntax already covered by preprocessor compatibility
  flags.

## Deliverables
- A reduced preprocessor unit test proving deferred function-like macro names
  rescan at expansion time.
- Preprocessor fix preserving hide-set termination.
- gcc-torture rerun for representative macro-heavy cases.

## Acceptance
- Representative case `20020108-1` no longer leaves raw `REPEAT_FN(...)`
  tokens for the parser.
- The fix does not reintroduce recursive macro expansion loops.

## Result
- Fixed logical token spelling recovery by stripping C phase-2
  backslash-newline splices when preprocessor expansion and phase-7 parser
  conversion recover token text from physical spans.
- Added a reduced preprocessor regression for an object-like macro body that
  contains CRLF-spliced `REPEAT_FN(...)` calls and must rescan into a
  function-like macro expansion.
- Added lexer and phase-7 parser regressions for splice stripping.
- WSL representative run: `gcc-torture::execute::20020108-1` passes.
- WSL full `gcc-torture` execute run: 1650 cases, 1137 pass, 513 fail,
  0 xfail, 0 skip; pass_rate=0.689.

## References
- `target/wsl/gcc-torture-15a-probes-after-alias.json`
- `target/wsl/gcc-torture-15f-20020108.json`
- `target/wsl/gcc-torture-full-15f-final.json`
