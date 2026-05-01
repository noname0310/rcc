# 05-33: Type-name validation contract

> ✓ done — 2026-05-01

**Phase:** 05-parse    **Depends on:** 05-32    **Milestone:** M2.1

## Goal
Make the boundary between parser syntax acceptance and later
constraint checking explicit for `type-name` contexts.

## Scope
- In:
  - Audit every `parse_type_name` caller: cast, `sizeof(type)`,
    compound literal, parameter abstract declarator, and future builtin
    type arguments.
  - Decide which invalid type-name forms are parser errors and which
    are HIR/typeck constraint errors.
  - Add tests for storage-class specifiers, `inline`, empty type-name,
    duplicate qualifiers, abstract declarator names, and typedef-name
    disambiguation.
  - Add helper API if needed so future parser extensions can parse a
    strict `type-name` without accepting declaration-only specifiers.
- Out:
  - Full type compatibility checking.
  - Layout or `sizeof` computation.

## Deliverables
- Tests around `parse_type_name` and expression contexts.
- Parser helper or documented contract in `crates/rcc_parse/src/decl.rs`.
- Follow-up notes for HIR/typeck if any constraint remains deferred.

## Acceptance
- `sizeof(static int)` and `(typedef int)x` style invalid type-name
  inputs are either rejected at parse time or covered by a documented
  downstream diagnostic test.
- Future builtin syntax can call a strict type-name parser without
  guessing which specifiers are allowed.
- No parser test relies on accepting declaration-only specifiers inside
  a type-name without a named downstream owner.

## References
- C99 §6.7.6 `type-name`.
- C99 §6.5.3.4 `sizeof`.
- C99 §6.5.4 cast.
- `crates/rcc_parse/src/decl.rs::parse_type_name`.
