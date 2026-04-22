# 04-04: Include-guard detection

**Phase:** 04-preprocess    **Depends on:** 04-03    **Milestone:** M5

## Goal
When `#include`ing a file, detect the idiomatic guard pattern and
cache its `FileId`: once the guard macro is defined, subsequent
includes of the same file are skipped without re-tokenising.

## Scope
- In: detect pattern `#ifndef X / #define X / ... / #endif` (first
  and last non-whitespace tokens); populate
  `Preprocessor::include_guards`.
- Out: full macro expansion (task 08).

## Deliverables
- `detect_guard(tokens: &[PpToken]) -> Option<Symbol>`.
- Unit test: `ok.h` with proper guard, `bad.h` with stray token
  before `#ifndef` (not detected — still processed fully).

## Acceptance
- Re-including a guarded header is an O(1) skip.
- A deliberately non-guarded header is still processed
  normally (and tests that exercise it pass).

## References
- Clang's `HeaderSearch` guard optimisation for prior art.
