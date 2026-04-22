# 05-23: `enum` + enumerators

**Phase:** 05-parse    **Depends on:** 05-18    **Milestone:** M4

## Goal
Parse `enum tag? { enumerator-list }` and bare `enum tag` references.
Enumerators are `IDENT` or `IDENT = const-expr`; trailing comma is
allowed (C99 §6.7.2.2p5).

## Scope
- In: enumerator list parsing; duplicate-name detection deferred to
  HIR; enumerator value expressions stored raw.
- Out: underlying-integer-type selection (HIR).

## Deliverables
- `parse_enum_spec()`.
- Tests incl. empty body (error per standard), trailing comma,
  bare reference.

## Acceptance
- `enum { A = 1, B, C = 10 }` parses with 3 enumerators, middle
  one carrying no explicit value.

## References
- C99 §6.7.2.2.
