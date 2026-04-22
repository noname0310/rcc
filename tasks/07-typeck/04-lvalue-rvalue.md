# 07-04: lvalue / rvalue and the L→R conversion

**Phase:** 07-typeck    **Depends on:** 07-03    **Milestone:** M3

## Goal
Compute `HirExpr::value_cat` for every expression per C99 §6.3.2.1,
and insert `LvalueToRvalue` conversion everywhere an rvalue is
required (any operand of an arithmetic operator, right-hand side of
`=`, function argument, etc.).

## Scope
- In: classification rules for each `HirExprKind`; propagation through
  `Convert`, `Paren`-equivalents; emit E0080 "assignment to rvalue"
  when the LHS of `=` is not an lvalue.
- Out: modifiable-lvalue discrimination (C99 §6.3.2.1p1 "modifiable";
  handled inside the assignment constraint task 05).

## Deliverables
- `value_category(expr) -> ValueCat`.
- Fixtures asserting category on every `HirExprKind` arm.

## Acceptance
- `int x; x = 1;` — lhs is lvalue, rhs is rvalue after conversion.
- `(int)x = 1;` → E0080.

## References
- C99 §6.3.2.1.
