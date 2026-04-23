> ✓ done — 2026-04-23

# 05-18: Declaration specifiers

**Phase:** 05-parse    **Depends on:** 05-02    **Milestone:** M1+

## Goal
Parse `declaration-specifiers`: any combination of storage class,
type qualifier, function specifier, and type specifier, in any order
per C99 §6.7. Combine into `DeclSpecs`.

## Scope
- In: loop consuming specifiers while they make sense; emit E0060 on
  conflicting storage (e.g. `static extern`); delegate struct/union
  (task 22) and enum (task 23) to sub-parsers.
- Out: declarator list parsing (task 19).

## Deliverables
- `parse_decl_specs() -> DeclSpecs`.
- Exhaustive fixture: `static const unsigned long long`, `inline extern`,
  `typedef struct S { ... } S`.

## Acceptance
- `const volatile int` accepted; `const const` accepted with a
  warning W0004 (C99 allows redundant quals but it smells).
- `short long` rejected with E0061.

## References
- C99 §6.7.
