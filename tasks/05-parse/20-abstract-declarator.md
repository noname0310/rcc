> ✓ done — 2026-04-23

# 05-20: Abstract declarators

**Phase:** 05-parse    **Depends on:** 05-19    **Milestone:** M1+

## Goal
Parse declarators with **no** identifier — needed for `type-name`
(§6.7.6) in `sizeof(T)`, casts, parameter types, and compound
literals.

## Scope
- In: share the chain-parsing with task 19; `Declarator::name = None`
  indicates abstract; error E0062 if a name appears where abstract
  was expected.
- Out: K&R param lists (task 26).

## Deliverables
- `parse_abstract_declarator() -> Declarator`.
- `parse_type_name() -> TypeName` wrapping specs + abstract decl.

## Acceptance
- `int (*)(int)` parses; `int foo(int)` does not when abstract was
  requested.
- Parameter of `void f(int, char*)` parses with two abstract
  declarators.

## References
- C99 §6.7.6.
