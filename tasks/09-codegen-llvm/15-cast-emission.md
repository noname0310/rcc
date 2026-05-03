> ✓ done — 2026-05-04

# 09-15: Cast emission

**Phase:** 09-codegen-llvm    **Depends on:** 09-05, 09-09, 09-14    **Milestone:** M3

## Goal

Lower every `rcc_cfg::CastKind` to the correct LLVM instruction sequence using
source and target type information.

## Scope

- In: int-int trunc/zext/sext, int-float, float-int, float-float, pointer-pointer,
  pointer-int, integer-pointer, `_Bool` normalization, and no-op casts.
- Out: real/complex conversions; owned by 09-18.

## Deliverables

- `emit_cast` helper.
- Tests for signedness, narrowing, widening, and pointer casts.

## Acceptance

- Cast result LLVM type exactly matches the destination `TyId`.
- Signedness decisions come from HIR type, not integer value.

## References

- `rcc_cfg::CastKind`
- C99 6.3
