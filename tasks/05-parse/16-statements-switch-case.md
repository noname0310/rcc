> ✓ done — 2026-04-23

# 05-16: `switch`, `case`, `default`

**Phase:** 05-parse    **Depends on:** 05-13    **Milestone:** M3

## Goal
Parse `switch (expr) stmt`, `case const-expr : stmt`, `default : stmt`.
Labels are statements; they wrap an inner statement.

## Scope
- In: recursive; no special check for `case` outside `switch` at
  parse time (HIR lower reports that); allow `default` before any
  `case` (still legal).
- Out: constant-expression evaluation (typeck).

## Deliverables
- Parser branches.
- Fixture with fallthrough, nested switch.

## Acceptance
- Nested switch parses with each `case` attached to the correct
  switch body (scope bookkeeping).

## References
- C99 §6.8.4.2.
