# 11-15s1: GNU vector type and layout

**Phase:** 11-conformance    **Depends on:** 11-15s    **Milestone:** M6

## Goal
Represent GNU `__attribute__((vector_size(N)))` as a first-class HIR type with
correct `sizeof` and alignment.

## Scope
- In: typedef/declaration `vector_size` on scalar integer and floating types.
- In: attribute size expressions with integer constants and `sizeof(type)`.
- Out: vector operators, ABI, and codegen arithmetic.

## Deliverables
- `Ty::Vector { elem, lanes, bytes }` or equivalent.
- HIR lowering for `vector_size`.
- Layout support for vector size/alignment.
- Parser/HIR/typeck tests for `typedef int v4si __attribute__((vector_size(16)))`.

## Acceptance
- `sizeof(v4si) == 16` and lane count is 4 for 32-bit `int`.
- Invalid vector sizes emit a diagnostic and do not silently fall back to scalar.
- Existing record/array/function layout tests still pass.

## References
- `docs/gnu-vector-design.md`
- `gcc-torture::execute::20050316-1`
