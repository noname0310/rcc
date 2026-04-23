> ✓ done — 2026-04-23

# 04-02: Directive parser

**Phase:** 04-preprocess    **Depends on:** 04-01    **Milestone:** M1+

## Goal
Classify an opening `#` line into one of the `Directive` variants. The
body tokens are kept raw for directive-specific processors (tasks 03,
06, 13, ...) to consume.

## Scope
- In: match the identifier after `#` against `include`, `define`,
  `undef`, `if`, `ifdef`, `ifndef`, `elif`, `else`, `endif`, `line`,
  `error`, `pragma`; unknown name = E0020.
- Out: evaluating the body (task-specific).

## Deliverables
- `parse_directive(line: &[PpToken]) -> Result<Directive, Diagnostic>`.
- Unit tests covering every variant + one negative case per
  malformed directive.

## Acceptance
- `#` followed by a non-keyword identifier produces E0020 with a
  `help: unknown preprocessing directive` suggestion.
- `#` alone (null directive, §6.10.7) parses as a `Directive::Pragma`
  with empty body (legal per standard).

## References
- C99 §6.10 lead + §6.10.7 "null directive".

## Notes (agent)

Two small deviations from the literal task text, flagged here so a
future reader does not think them accidental:

- **Error code for unknown directive is E0019, not E0020.** The task
  spec names E0020, but `rcc_errors::codes` already carries
  `E0019 = "unknown preprocessor directive"` (exact semantic match)
  and `E0020 = "#error directive encountered"` (reserved for task
  04-16). Reusing E0019 avoids stepping on task 04-16's slot and
  keeps the E0001..E0020 PP block intact. If preferred, swap the
  two code slots in a follow-up `meta:` commit.

- **`parse_directive` takes `(line, src, interner)`, not just `line`.**
  `PpToken` carries only a `Span` (no text), so directive-name
  classification, `Symbol` creation for `#undef`, and raw-text
  materialisation for `#include` / `#error` all require the file
  source and the session interner. The expanded signature is
  documented on the function itself.
