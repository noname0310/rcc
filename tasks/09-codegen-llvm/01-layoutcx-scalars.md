# 09-01: Scalar layout

**Phase:** 09-codegen-llvm    **Depends on:** —    **Milestone:** M3

## Goal
Verify and extend `LayoutCx::layout_of` for every scalar `Ty`:
`void`, `_Bool`, `char`, `short`, `int`, `long`, `long long`,
`float`, `double`, `long double`, pointers. Matches System V x86-64.

## Scope
- In: unit tests asserting `size` / `align`; fix `long double` to
  16-byte size with 16-byte align on System V (skeleton currently
  says 16/16 — good, but double-check).
- Out: aggregate layout (tasks 02/03).

## Deliverables
- Updated `layout.rs`.
- Fixture test with all scalar kinds.

## Acceptance
- `layout_of(TyCtxt::double) == Layout { size: 8, align: 8 }`.
- `layout_of(pointer type) == Layout { size: 8, align: 8 }` on
  x86-64.

## References
- System V ABI, section 3.1.2.
