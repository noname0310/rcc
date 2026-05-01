> ✓ done — 2026-05-01

# 06-14: central `DeclSpecs` / `TypeName` to `TyId` service

**Phase:** 06-hir-lower    **Depends on:** 06-13    **Milestone:** M5 stabilization

## Goal
Replace the scattered and partial specifier lowering paths with one
central service that converts AST declaration specifiers plus an optional
declarator into a complete HIR `TyId`.

## Scope
- In: `DeclSpecs` base type lowering for scalar builtins, typedef names,
  `struct` / `union`, `enum`, qualifiers, and declarator folding.
- In: a public or crate-private helper such as
  `lower_type_from_parts(specs, declarator, scope, ...) -> TyId`.
- In: all existing call sites that currently use
  `lower_declspecs_to_base_ty`, `lower_block_specs_to_base_ty`, or
  `lower_field_specs_to_base_ty`.
- Out: compound literal temporary materialization and switch case
  collection; those are later tasks.

## Deliverables
- One canonical type lowering entry point in `rcc_hir_lower`.
- `lower_declspecs_to_base_ty` stops silently mapping unsupported specs
  to `tcx.int`.
- Field, parameter, function return, local declaration, and type-name
  lowering call the same service.
- Unit tests for every accepted specifier category.

## Acceptance
- `typedef long T; T x;` lowers `x` as `long`, not `int`.
- `struct S { int a; }; struct S s;` lowers `s` as `Ty::Record`.
- `enum E { A }; enum E e;` lowers `e` as `Ty::Enum` or the chosen enum
  representation consistently with the HIR design.
- `unsigned long *p;` still lowers through the same helper.
- Unsupported specifier combinations emit diagnostics or `tcx.error`;
  they must not silently become `int`.

## References
- C99 §6.7.2 — Type specifiers.
- `crates/rcc_hir_lower/src/lib.rs`:
  `lower_declspecs_to_base_ty`, `lower_block_specs_to_base_ty`,
  `lower_field_specs_to_base_ty`, `apply_declarator`.
