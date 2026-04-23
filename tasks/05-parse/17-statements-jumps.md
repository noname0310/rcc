> ✓ done — 2026-04-23

# 05-17: Jump statements

**Phase:** 05-parse    **Depends on:** 05-13    **Milestone:** M1+

## Goal
Parse `break`, `continue`, `return [expr]`, `goto label`, and
labelled statements `label: stmt`.

## Scope
- In: `return` with no expression → `StmtKind::Return(None)`;
  labelled statements recognised by lookahead (Ident `:`).
- Out: checking that labels resolve (HIR / CFG).

## Deliverables
- Parser branches.
- Tests per form.

## Acceptance
- `goto end; end: ;` parses with the label statement wrapping the
  null statement.
- `break;` outside a loop parses (diagnostic is HIR's job later).

## References
- C99 §6.8.1, §6.8.6.
