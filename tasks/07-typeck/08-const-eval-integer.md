# 07-08: Integer constant-expression evaluator

**Phase:** 07-typeck    **Depends on:** 07-01    **Milestone:** M3

## Goal
Replace the `rcc_typeck::const_eval::ConstEval::eval` stub with a
full integer evaluator for C99 §6.6 "integer constant expressions":
literals, enumerators, sizeof, every operator except assignment /
comma / function call.

## Scope
- In: operations on i128 / u128 with overflow detection; emit E0083
  on overflow in non-assignment context (UB but diagnosing is kind).
- Out: float const-eval (task 09).

## Deliverables
- `ConstEval::eval_int(expr) -> Option<i128>` rewrite.
- Fixture: exhaustive operator table.

## Acceptance
- `1 + 2 * 3` evaluates to 7.
- `sizeof(int)` evaluates to the target's sizeof(int).
- INT_MAX + 1 → overflow diagnostic.

## References
- C99 §6.6.
