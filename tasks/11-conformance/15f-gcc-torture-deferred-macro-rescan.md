# 11-15f: gcc-torture deferred macro rescan

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

## References
- `target/wsl/gcc-torture-15a-probes-after-alias.json`
