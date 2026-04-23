> ✓ done — 2026-04-23

# 05-21: Typedef-name disambiguation

**Phase:** 05-parse    **Depends on:** 05-18, 05-19    **Milestone:** M1+

## Goal
Resolve the classical ambiguity between `identifier` and
`typedef-name` using `ScopeStack`. When the parser reaches a
potential declaration-specifier slot or a type-name slot, it peeks
the next `Ident` and asks the scope stack whether it's a typedef.

## Scope
- In: declare `typedef T` → `scope.declare(T, NameKind::Typedef)` at
  the moment the top-level declarator is complete; restore scope on
  block exit; test the edge case "`typedef int T; { int T; T x; }"
  where inner `T` shadows the typedef.
- Out: non-trivial name resolution (HIR's job).

## Deliverables
- Scope updates wired into `Parser::declare_from_specs`.
- Regression tests for each classic case in Harbison-Steele Table 4-2.

## Acceptance
- `typedef int T; T x;` parses as declaration of `x` with type `T`.
- `typedef int T; int T; T x;` parses with `T` **not** a type at
  `T x;` (shadowed).

## References
- C99 §6.7.7 (typedef).
- "The Lexer Hack" (Wikipedia) for the classic treatment.
