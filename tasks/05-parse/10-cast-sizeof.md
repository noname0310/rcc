# 05-10: Cast, `sizeof`, compound literals

**Phase:** 05-parse    **Depends on:** 05-20, 05-24    **Milestone:** M1+ / M6

## Goal
Parse the three constructs that begin with `(` and require type-name
disambiguation: cast `(T)e`, `sizeof(T)`, and C99 compound literal
`(T){ init }`.

## Scope
- In: two-token lookahead plus scope-aware "is this a typedef-name?"
  test (task 21); produce `ExprKind::Cast`, `SizeofType`, or
  `CompoundLiteral` accordingly.
- Out: `_Alignof` (C11, not in C99).

## Deliverables
- `parse_paren_or_cast()` decision fn.
- Tests: `(int)x`, `sizeof(int)`, `sizeof x`, `(int[3]){0}`.

## Acceptance
- Ambiguity with a typedef'd type: `typedef int T; (T)x` parses as
  cast; `int T = 0; (T)` parses as paren-group.
- Compound literal with nested designators parses (full exercise in
  task 24).

## References
- C99 §6.5.3.4 (`sizeof`), §6.5.4 (cast), §6.5.2.5 (compound literal).

## Notes (agent)
- 2026-04-23: skipped in favour of 05-11 — upstream deps 05-20 (abstract-declarator) and 05-24 (init-list-designators) are still `[ ]` in `tasks/05-parse/index.md`, so this task's type-name lookahead has nothing to call into. Will be picked up after both land.
