> ✓ done — 2026-04-24

# 05-10b: Compound literals `(T){ init }`

**Phase:** 05-parse    **Depends on:** 05-10, 05-24    **Milestone:** M1+ / M6

## Goal
Finish the remaining slice of the original task 05-10: parse C99
compound literals (§6.5.2.5) — a postfix-expression of the shape
`( type-name ) { initializer-list }` that produces an unnamed object
of the given type. Task 05-10 delivered the other two `(`-initial
constructs (cast `(T)e` and `sizeof(T)`); compound literals were
split out because they also need the initializer-list parser, which
is owned by task 05-24.

## Scope
- In: recognise `( type-name ) {` as a compound-literal prefix in
  `parse_prefix_unary` (or in a dedicated `parse_compound_literal`
  helper called from it), parse the wrapped type-name via
  [`parse_type_name`] (already public since 05-20), call
  `parse_initializer` (task 05-24) for the braced body, and produce
  `ExprKind::CompoundLiteral { ty, init }`.
- In: wire the new arm through postfix parsing too if needed — per
  C99 §6.5.2 the compound literal is a *postfix*-expression, so the
  result must itself accept `.field` / `->field` / `[idx]` / `(args)`
  / `++` / `--` chains (`((T){0}).x`).
- Out: any further cast / sizeof / type-name work (owned by 05-10).
- Out: initializer-list parsing internals (owned by 05-24).

## Key decision — disambiguation
Task 05-10 already established the lookahead that fires on a `(`
followed by a *type-specifier keyword* or a *typedef-name* (via
`ScopeStack::is_typedef`). This task adds the third branch: after
the closing `)`, if the next token is `{`, build a compound literal
instead of a cast. The shape is:

```text
   ( type-name ) {    →   ExprKind::CompoundLiteral
   ( type-name ) e    →   ExprKind::Cast           (task 05-10)
   ( expression ) …   →   ExprKind::Paren          (primary)
```

The decision is deterministic once the closing `)` is past — no
extra lookahead beyond what 05-10 already does.

## Deliverables
- Extend `parse_prefix_unary` / add `parse_compound_literal` so that
  `( type-name ) {` builds `ExprKind::CompoundLiteral` and then re-
  enters the postfix loop for trailing `.` / `->` / `[idx]` / etc.
- Tests:
  - `(int[3]){0}` parses to a compound literal with an `int[3]` type
    and a single-element initializer list.
  - `(struct S){.x = 1}` parses — designator inside.
  - `((T){0}).x` parses — postfix `.x` on the literal.
  - Regression: the cast and sizeof tests from task 05-10 still pass
    unchanged (the new branch only fires when `{` follows `)`).

## Acceptance
- Compound literal with nested designators parses (full exercise
  threaded through task 05-24's initializer machinery).
- `(int[3]){0}` parses; the original task-05-10 acceptance fixture
  now reaches green.
- `parse_type_name` / `parse_abstract_declarator` are unchanged —
  this task only edits the expression side.

## References
- C99 §6.5.2.5 (compound literal).
- Task [`10-cast-sizeof`](10-cast-sizeof.md) — the cast and sizeof
  sibling that landed ahead of this split.
- Task [`24-init-list-designators`](24-init-list-designators.md) —
  owns `parse_initializer`, which this task calls into.

## Notes (agent)
- 2026-04-23: created by splitting task 05-10. The original task
  bundled cast + sizeof + compound literal, but compound literal
  depends on initializer parsing (task 05-24) while cast and sizeof
  only need the abstract declarator (task 05-20). Once 05-20 landed
  we ran task 10 on its own for the two ready pieces; this file
  captures what remains.
