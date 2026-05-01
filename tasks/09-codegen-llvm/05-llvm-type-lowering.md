> ✓ done — 2026-05-02

# 09-05: LLVM type lowering

**Phase:** 09-codegen-llvm    **Depends on:** 09-02, 09-03, 09-04    **Milestone:** M3

## Goal

Add a single `TyId -> LLVM type` service so all later codegen tasks share the
same scalar, pointer, array, record, function, and complex representation.

## Scope

- In: recursive type cache, opaque placeholders for recursive records, function
  types, void, pointers, fixed arrays, structs/unions, and `_Complex`.
- Out: ABI coercion types; owned by 09-07 and 09-08.

## Deliverables

- `TypeCx` or equivalent helper inside `rcc_codegen_llvm`.
- Tests that recursive structs terminate and repeated queries reuse types.

## Acceptance

- Every non-`Ty::Error` HIR type either lowers to an LLVM type or returns a
  structured backend error with the original `TyId`.
- Function declarations can request an LLVM function type without body codegen.

## References

- LLVM LangRef: Type system
- `rcc_hir::Ty`
