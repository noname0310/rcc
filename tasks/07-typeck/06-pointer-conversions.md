# 07-06: Pointer conversions

**Phase:** 07-typeck    **Depends on:** 07-05    **Milestone:** M3

## Goal
Insert `ConvertKind::Pointer` around pointer expressions that need to
be converted to/from `void*`, compatible-pointee types, or null pointer
constants. Follows C99 §6.3.2.3.

## Scope
- In: rules for `void*` bidirectional conversion; qualifier addition
  (`T*` → `const T*`) OK, removal requires explicit cast; function-
  pointer only converts with compatible signature (else E0082).
- Out: pointer arithmetic (handled in CFG phase as `PtrAdd`).

## Deliverables
- `pointer_convert(src_ty, dst_ty, expr) -> Option<HirExpr>`.
- Truth-table test covering every bullet of §6.3.2.3.

## Acceptance
- `void *p = &x;` accepted.
- `int *p = &x; char *q = p;` → E0082 without explicit cast.

## References
- C99 §6.3.2.3.
