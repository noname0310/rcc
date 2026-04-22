# 03-02: Line splicing (C99 translation phase 2)

**Phase:** 03-lex    **Depends on:** 03-01    **Milestone:** M1

## Goal
Implement backslash-newline removal (C99 §5.1.1.2 phase 2). A source
line ending in `\` followed by `\n` must be joined with the next line
**before** any other tokenisation step sees it. Spans must still
point at the *physical* byte ranges in the original file so
diagnostics underline correctly.

## Scope
- In: a pre-pass (or streaming adapter) over the char cursor that
  skips `\\\n` without shifting the underlying `BytePos`; preserve a
  mapping so `Span` values remain valid.
- Out: trigraph substitution (C99 §5.1.1.2 phase 1; deprecated and
  **not** implemented — out of scope for the whole phase).

## Deliverables
- New `LineSpliceCursor` (or equivalent) wrapping the base cursor.
- Fixture test: `"abc\\\ndef"` yields an identifier `abcdef` whose
  span covers the full source slice including the `\\\n`.

## Acceptance
- A token spanning a splice carries a span that, when rendered by
  `StderrEmitter`, underlines both physical lines (multi-line label).
- Passes on `#define FOO \\\n bar` where the splice is *inside* a
  directive (smoke test — exercised fully in 04-preprocess).

## References
- C99 §5.1.1.2 phase 2.
