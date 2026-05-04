# 11-16d: tcc-tests2 macro empty arguments

**Phase:** 11-conformance    **Depends on:** 11-16    **Milestone:** M6

## Goal
Fix C99 macro expansion when an argument is intentionally empty.

## Scope
- In: `tcc-tests2::71_macro_empty_arg`.
- Out: GNU comma elision beyond already implemented compatibility flags.

## Deliverables
- Preprocessor unit tests for empty actual arguments, stringize, paste, and
  rescanning.
- A fix that preserves C99 empty arguments without dropping separators or
  corrupting hide-sets.

## Acceptance
- `71_macro_empty_arg` passes through tcc-tests2.
- Existing chibicc macro and preprocess expansion-matrix tests still pass.

## References
- `target/wsl/tcc-tests2-16-final.json`
- C99 §6.10.3.
