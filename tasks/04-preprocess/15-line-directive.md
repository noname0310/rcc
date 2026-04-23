> ✓ done — 2026-04-23

# 04-15: `#line`

**Phase:** 04-preprocess    **Depends on:** 04-02    **Milestone:** M5

## Goal
Handle `#line N` and `#line N "file"` per C99 §6.10.4: subsequent
`__LINE__` expansions return the overridden number + 1 per newline;
`__FILE__` returns the overridden name if supplied.

## Scope
- In: maintain a `LineMap` layered over `SourceMap` that records
  overrides; the lexer is unchanged (it keeps physical positions).
- Out: generated-code scenarios where `#line` drops us into a file
  the `SourceMap` does not know about — we synthesise a virtual
  `SourceFile` with `src = ""` and only the override name.

## Deliverables
- `Preprocessor::line_overrides: Vec<(FileId, Override)>`.
- Tests: `#line 100`, `#line 100 "foo"`, invalid `#line abc`.

## Acceptance
- After `#line 100 "foo.c"`, the next `__LINE__` is `100` and
  `__FILE__` is `"foo.c"`.
- `#line 0` is rejected (standard requires a nonzero line number).

## References
- C99 §6.10.4.
