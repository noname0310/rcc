> ✓ done — 2026-04-23

# 05-22: `struct` / `union` specifier + fields

**Phase:** 05-parse    **Depends on:** 05-18, 05-19    **Milestone:** M4

## Goal
Parse `struct tag? { field-decl* }` and bare `struct tag` references.
Field declarations reuse `DeclSpecs` + a list of `FieldDeclarator`s
(which may include bitfield widths).

## Scope
- In: recognise struct vs union keyword; optional tag; field list;
  anonymous bitfields (`int : 3`); incomplete struct declarations
  (`struct S;` with no body).
- Out: layout / offset calculation (codegen).

## Deliverables
- `parse_record_spec()` returning `RecordSpec`.
- Tests: nested struct, bitfield, `struct { ... }` anonymous,
  recursive `struct Node { struct Node *next; }`.

## Acceptance
- Bitfield width is a C99 constant-expression (parsed; evaluated
  later).
- Flexible array member (`T data[];` at end of struct) parses (C99
  §6.7.2.1p16).

## References
- C99 §6.7.2.1.
