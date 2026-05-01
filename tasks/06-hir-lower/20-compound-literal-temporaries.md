# 06-20: compound literal temporary objects

**Phase:** 06-hir-lower    **Depends on:** 06-19    **Milestone:** M5 stabilization

## Goal
Lower C99 compound literals into real HIR storage instead of
`IntConst(0)` placeholders.

## Scope
- In: block-scope compound literals as synthetic locals with automatic
  storage duration.
- In: file-scope compound literals as synthetic internal globals when
  needed by constant initialization.
- In: scalar, array, record, and typedef-named compound literal types.
- In: brace initializer reuse through `lower_initializer`.
- Out: non-standard GNU compound literal extensions.

## Deliverables
- A HIR representation for the compound literal lvalue.
- Synthetic local/global naming that is stable in tests.
- Initializer lowering wired to the synthetic object.
- Typeck and CFG support for the produced shape.

## Acceptance
- `int *p = &(int){3};` creates a temporary lvalue object.
- `return ((struct S){ .x = 1 }).x;` lowers through record initializer
  machinery.
- `(int[3]){1,2,3}[1]` is indexable as an array lvalue.
- The old `expr_compound_literal_placeholder` test is replaced.

## References
- C99 §6.5.2.5 — Compound literals.
- `lower_expr` currently placeholder-lowers `CompoundLiteral`.

