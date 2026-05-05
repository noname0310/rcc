> ✓ done — 2026-05-05

# 14-01: `_Pragma` operator

**Phase:** 14-lang-extensions    **Depends on:** —    **Milestone:** M5

## Goal
Implement the C99 `_Pragma` operator (C99 §6.10.9). Parse
`_Pragma(string-literal)` in the preprocessor, destringize the
string literal, and feed the result as a `#pragma` directive to
the pragma handler.

## Scope
- In: tokenizer recognition of `_Pragma` keyword, string
  destringization (unescape `\"` → `"`, `\\` → `\`), forwarding
  to the existing `#pragma` handling path.
- Out: semantic handling of individual pragma directives (existing
  or future tasks).

## Deliverables
- `_Pragma` recognition in `rcc_preprocess`.
- Destringization helper function.
- Tests: `_Pragma("once")`, `_Pragma("GCC diagnostic push")`.

## Acceptance
- `_Pragma("once")` prevents double-inclusion in a test case.
- Malformed `_Pragma` (missing parens, non-string argument) emits
  a diagnostic.

## References
- C99 §6.10.9 — `_Pragma` operator.
