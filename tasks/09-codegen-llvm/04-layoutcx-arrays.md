> ✓ done — 2026-05-04

# 09-04: LayoutCx arrays, FAM, and VLA sentinel

**Phase:** 09-codegen-llvm    **Depends on:** 09-02, 09-03    **Milestone:** M4

## Goal

Compute layout for fixed arrays, incomplete arrays, flexible array members,
and the compile-time layout sentinel used by VLA codegen.

## Scope

- In: fixed-length array size/align, array-of-records, incomplete array errors,
  FAM tail layout, and `is_vla = true` sentinel layout.
- Out: dynamic VLA allocation and `sizeof(VLA)`; owned by 09-17.

## Deliverables

- `LayoutCx` tests for scalar arrays, record arrays, FAM records, and VLA.
- Documentation in the task file explaining why VLA has no static size.

## Acceptance

- Fixed array size is `elem.size * len` with overflow checked.
- VLA layout returns element alignment while refusing to claim a static byte
  size for allocation.

## Notes

VLA element layout is known at compile time, so codegen can still use the
element alignment when creating dynamic stack storage. The byte size is not a
static layout property because it depends on the runtime bound saved by CFG
lowering; `LayoutCx::array_layout_of` therefore reports `static_size = None`
for VLA sentinels, while `layout_of` continues to reject VLA objects as
unsized.

## References

- C99 6.7.5.2
