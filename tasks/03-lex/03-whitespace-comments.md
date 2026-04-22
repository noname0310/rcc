# 03-03: Whitespace and comments

**Phase:** 03-lex    **Depends on:** 03-02    **Milestone:** M1

## Goal
Skip runs of horizontal whitespace, emit `PpTokenKind::Newline` per
physical newline (the preprocessor needs these to detect directive
boundaries), and reduce comments to a single space (§6.4p3).

## Scope
- In: horizontal whitespace run collapsing; `//` to EOL; `/* ... */`
  with nested-comment rejection (C has no nesting — emit E0003 if
  the content contains `/*`); EOF inside `/*` = error E0004.
- Out: emit `Whitespace` tokens (we drop them — downstream crates do
  not care unless `--emit=tokens` wants them; expose a config flag
  `preserve_whitespace` for the `--emit=tokens` mode).

## Deliverables
- Lexer emits `Newline` + optional `Whitespace`.
- Errors E0003, E0004 registered per task 02-02.
- Fixture tests for each comment style including nested `/*`.

## Acceptance
- Lexing `/* a */ b /* c */` yields: `[/* */ (skipped)] Ident("b") [/* */ (skipped)]`
  with single-character start/end spans intact.
- Unclosed `/*` produces E0004 diagnostic with label at the opening.

## References
- C99 §6.4p3 (comment → single space).
- C99 §5.1.1.2 phase 3.
