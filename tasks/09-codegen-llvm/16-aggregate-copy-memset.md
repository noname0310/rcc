> ✓ done — 2026-05-04

# 09-16: Aggregate copy and memset intrinsics

**Phase:** 09-codegen-llvm    **Depends on:** 09-09, 09-12    **Milestone:** M4

## Goal

Emit aggregate moves and zero-initialization using LLVM `memcpy` / `memset`
intrinsics instead of scalarizing large objects too early.

## Scope

- In: `ConstKind::ZeroInit`, aggregate `Rvalue::Use`, struct/array local init,
  alignment operands, and volatile=false default.
- Out: volatile aggregate copy; owned by 09-20 if required.

## Deliverables

- `emit_memcpy` and `emit_memset` helpers.
- Tests for struct assignment, array zero-init, and nested aggregate copy.

## Acceptance

- LLVM IR contains typed intrinsic declarations accepted by LLVM 18.
- Aggregate copies preserve exact byte size from `LayoutCx`.

## References

- LLVM LangRef: `llvm.memcpy`, `llvm.memset`
