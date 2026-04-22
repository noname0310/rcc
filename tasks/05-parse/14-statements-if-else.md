# 05-14: `if` / `else`

**Phase:** 05-parse    **Depends on:** 05-13    **Milestone:** M1+

## Goal
Parse `if (cond) stmt` and `if (cond) stmt else stmt`, resolving the
"dangling else" by binding `else` to the nearest preceding `if`.

## Scope
- In: standard recursive parse; greedy `else` consumption.
- Out: --.

## Deliverables
- Branch in `parse_stmt`.
- Tests including `if (a) if (b) x; else y;` (must bind `else` to
  inner `if`).

## Acceptance
- AST matches expected nesting on the dangling-else fixture.

## References
- C99 §6.8.4.1.
