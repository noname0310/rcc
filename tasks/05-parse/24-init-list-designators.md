# 05-24: Initializer lists + designators (C99)

**Phase:** 05-parse    **Depends on:** 05-19    **Milestone:** M4 / M6

## Goal
Parse initializers: bare expression, `{ expr-list }`, or designated
list `{ .x = 1, [2] = 3, [5] .sub = 7 }` (C99 §6.7.8).

## Scope
- In: recursive `Initializer`; designator chain `.ident` / `[expr]`
  before `= value`; empty `{}` is error per C99 (gcc extension; we
  reject per spec).
- Out: initializer flattening into zero-init + per-field stores
  (HIR / CFG).

## Deliverables
- `parse_initializer() -> Initializer`.
- Test cases matching §6.7.8 examples 1, 2, 11, 17.

## Acceptance
- Nested designators `{[0].x = 1}` parse into the expected tree.
- Mixing positional and designated entries (permitted by C99)
  parses.

## References
- C99 §6.7.8.
