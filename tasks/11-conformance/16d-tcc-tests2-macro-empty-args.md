> ✓ done — 2026-05-04

# 11-16d: tcc-tests2 macro empty arguments

**Phase:** 11-conformance    **Depends on:** 11-16    **Milestone:** M6

## Goal
Classify `71_macro_empty_arg` accurately: the macro expansion is correct,
and the remaining mismatch is a vendored `.expect` final-newline drift.

## Scope
- In: `tcc-tests2::71_macro_empty_arg`.
- Out: GNU comma elision beyond already implemented compatibility flags.

## Deliverables
- Preprocessor unit tests for empty actual arguments, stringize, paste, and
  rescanning.
- A case-specific tcc-tests2 comparator normalization for the missing final
  newline, with host-compiler evidence that the source prints no newline.

## Acceptance
- `71_macro_empty_arg` passes through tcc-tests2.
- Existing chibicc macro and preprocess expansion-matrix tests still pass.

## References
- `target/wsl/tcc-tests2-16d-empty-arg.json`
- `target/wsl/tcc-tests2-16d-full.json`
- C99 §6.10.3.
