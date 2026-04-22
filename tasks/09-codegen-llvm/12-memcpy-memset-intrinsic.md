# 09-12: `memcpy` / `memset` intrinsics

**Phase:** 09-codegen-llvm    **Depends on:** 09-06    **Milestone:** M4

## Goal
Emit `llvm.memcpy.p0i8.p0i8.i64` / `llvm.memset.p0i8.i64` for:
- Aggregate copies (`struct X a = b;`).
- `ConstKind::ZeroInit` initialization (from CFG task 11).
- Array passing into a `byval` argument.

## Scope
- In: intrinsic builders; pick size parameter from layout; set
  alignment attribute.
- Out: custom copy loops (LLVM optimises `memcpy` itself).

## Deliverables
- Helpers `emit_memcpy`, `emit_memset`.
- Fixture: struct copy; array zero-init.

## Acceptance
- `int a[1024] = {0};` emits `memset` of size 4096.

## References
- LLVM LangRef intrinsics.
