# 09-19: Bitfield access codegen

**Phase:** 09-codegen-llvm    **Depends on:** 09-02    **Milestone:** M5

## Goal
Generate correct load and store sequences for bitfield members.
Bitfield read: load the containing storage unit, shift right, and
mask. Bitfield write: load, clear the target bits, shift-or the new
value, and store.

## Scope
- In: use bitfield layout information from `LayoutCx` (task 09-02)
  to determine the storage unit offset, bit offset within the unit,
  and bit width. Generate the shift/mask sequences for both read
  and write. Handle signed bitfields (sign-extend on read). Handle
  bitfields that do not cross storage unit boundaries.
- Out: bitfields crossing storage unit boundaries (ABI-dependent
  edge case — defer or emit diagnostic). Bitfield access via
  volatile pointer.

## Deliverables
- Bitfield load codegen: load + lshr + and (+ sext for signed).
- Bitfield store codegen: load + and-clear + shl-or + store.
- Tests: read/write bitfields of various widths and positions.

## Acceptance
- `struct { int a:3; int b:5; }; s.b = 15; return s.b;` compiles
  and the linked program returns 15.
- Signed bitfield `int x:4 = -3; return x;` returns -3.
- Adjacent bitfields do not corrupt each other on write.

## References
- C99 §6.7.2.1 — Structure and union specifiers (bitfields).
- System V ABI bitfield layout rules.
