> ??done ??2026-05-01

# 09-03: LayoutCx record layout

**Phase:** 09-codegen-llvm    **Depends on:** 09-02    **Milestone:** M4

## Goal

Compute struct/union size, alignment, field offsets, and bitfield storage
metadata consistently with the SysV x86-64 baseline.

## Scope

- In: normal fields, nested records, padding, unions, anonymous padding fields,
  and bitfield allocation metadata.
- Out: bitfield load/store instruction emission; owned by 09-21.

## Deliverables

- Record layout fixtures compared against host cc `offsetof` / `sizeof`.
- Tests for empty padding, nested struct, union max-size, and bitfield packs.

## Acceptance

- `DefKind::Record.layout` data is sufficient for GEP field addressing and
  later bitfield masking.
- Flexible array members contribute no trailing element size.

## References

- C99 6.7.2.1
- SysV x86-64 ABI aggregate layout
