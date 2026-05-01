# 05-40: C11 generic-selection syntax

**Phase:** 05-parse    **Depends on:** 05-39    **Milestone:** M6 blocker

## Goal
Parse `_Generic` expressions so C11 compatibility work and external
tests do not get blocked in the parser.

## Scope
- In:
  - Add AST representation for `_Generic(assignment-expression,
    generic-association-list)`.
  - Parse associations of `type-name : assignment-expression` and
    `default : assignment-expression`.
  - Preserve association order and spans.
  - Gate by language standard option once C11 mode exists.
- Out:
  - Type matching or selected expression evaluation.
  - C11 mode defaults outside parser syntax.

## Deliverables
- AST and parser support for generic selections.
- Tests for ordinary type associations, typedef-name associations,
  pointer types, and default association.
- Negative tests for duplicate defaults and malformed association
  separators if parser-level checking is chosen.

## Acceptance
- `_Generic(x, int: 1, default: 0)` parses under C11/extension mode.
- The first expression uses assignment-expression grammar, not full
  comma-expression grammar.
- Type-name associations use the same strict type-name contract from
  05-33.

## References
- C11 §6.5.1.1 generic selection.
- External C test suites that include C11 compatibility cases.
