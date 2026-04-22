# 07-09: Extended constant expression evaluator

**Phase:** 07-typeck    **Depends on:** 07-08    **Milestone:** M4

## Goal
Extend `ConstEval` to cover C99 §6.6p7 arithmetic-constant-expressions
(float) and §6.6p8 address-constants for global initializers
(`&arr[2]`, `(char*)0 + offset`).

## Scope
- In: evaluate f64 expressions; recognise address constants and
  emit `ConstValue::Int(address_as_int)` style placeholder referring
  to a global `DefId` plus offset.
- Out: runtime constant folding (M7 optimisation).

## Deliverables
- `ConstEval::eval_arith`, `ConstEval::eval_address`.
- Tests with global initializers from c-testsuite.

## Acceptance
- `static int arr[3] = {1, 2, 3}; int *p = &arr[2];` accepted; `p`
  resolves to `&arr + 2*sizeof(int)`.
- `1.0 / 3.0` evaluates in the context of a global init.

## References
- C99 §6.6.
