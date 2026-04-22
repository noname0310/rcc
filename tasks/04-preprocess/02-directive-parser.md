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
