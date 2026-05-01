# 06-18: complete record and enum tag definitions in place

> ✓ done — 2026-05-01

**Phase:** 06-hir-lower    **Depends on:** 06-17    **Milestone:** M5 stabilization

## Goal
Use `resolve_tag`, `lower_record`, and `lower_enum` together so tag
definitions and references point at one stable `DefId` with complete
fields or variants.

## Scope
- In: tagged `struct` / `union` / `enum` definitions at file, block,
  parameter, and field scope.
- In: anonymous records and enums when they appear as object types.
- In: forward declarations and later completion.
- In: duplicate tag completion diagnostics.
- Out: ABI layout caching; layout is handled by `rcc_hir::LayoutCx`.

## Deliverables
- A tag-materialization helper that returns `Ty::Record(def_id)` or
  `Ty::Enum(def_id)` from a `TypeSpec`.
- Existing placeholder tag defs are updated in place instead of replaced
  by duplicate defs.
- `lower_record` field specs can reference earlier tags and typedefs.

## Acceptance
- `struct S; struct S { int a; }; struct S s;` uses one `DefId` for all
  three mentions.
- `struct S { int a; }; struct S *p;` lowers pointer-to-record correctly.
- `struct A { struct B *b; }; struct B { int x; };` supports mutual
  references through pointers.
- `enum E { A = 1 }; enum E e;` exposes `A` in ordinary namespace and
  keeps the `E` tag complete.

## References
- C99 §6.7.2.1 — Structure and union specifiers.
- C99 §6.7.2.2 — Enumeration specifiers.
- Existing `resolve_tag`, `lower_record`, and `lower_enum` helpers.
