> ✓ done — 2026-04-24

# 06-07: Composite (struct / union) lowering

**Phase:** 06-hir-lower    **Depends on:** 06-03, 06-06    **Milestone:** M4

## Goal
Materialise `struct` / `union` definitions into `DefKind::Record` with
a `Vec<Field>`. Bitfield widths are constant-evaluated. Anonymous
fields inside structs are flattened into the parent for name lookup.

## Scope
- In: field lowering calls `apply_declarator` per declarator; bitfield
  width validation (`0..type_width`, E0077).
- Out: layout / offset computation (codegen).

## Deliverables
- `lower_record(spec: &RecordSpec, tcx, resolver) -> DefKind::Record`.
- Tests: bitfield width 0 (separator), negative width (error).

## Acceptance
- `struct { int a; struct { int b; }; }`: outer struct exposes both
  `a` and `b` for lookup.

## References
- C99 §6.7.2.1.
