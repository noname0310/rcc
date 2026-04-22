# 05-06: Adjacent string-literal concatenation

**Phase:** 05-parse    **Depends on:** 05-05    **Milestone:** M2

## Goal
Per C99 §6.4.5p4, adjacent string literals are concatenated into a
single token before parsing. Encoding-mismatched concatenation
(e.g. `L"a" "b"` OK; `L"a" U"b"` error E0041).

## Scope
- In: run after phase-7 conversion; produce one `StringLit` token
  whose span covers all contributing literals.
- Out: embedding in initializers (later tasks).

## Deliverables
- `merge_adjacent_strings(tokens: Vec<Token>) -> Vec<Token>` pass.
- Tests: narrow+narrow, narrow+wide, wide+narrow (allowed per §6.4.5p5),
  narrow+`U`-prefixed (error).

## Acceptance
- `"a" "b" "c"` produces `StringLit { bytes: "abc\\0", ... }`.
- Encoding promotion rule from §6.4.5p5 implemented (narrow + wide →
  wide).

## References
- C99 §6.4.5.
