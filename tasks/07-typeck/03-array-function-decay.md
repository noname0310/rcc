> ✓ done — 2026-04-26

# 07-03: Array / function decay

**Phase:** 07-typeck    **Depends on:** —    **Milestone:** M3

## Goal
Implement C99 §6.3.2.1p3 (array → pointer to element) and §6.3.2.1p4
(function → pointer to function). Conversion inserts a
`HirExprKind::Convert { kind: ArrayToPtr | FuncToPtr }` wrapper.

## Scope
- In: the insert happens on every use of an array / function lvalue
  except: operand of `sizeof`, operand of `&`, initialiser of a char
  array (C99 §6.3.2.1p3 exception).
- Out: lvalue-to-rvalue conversion (task 04).

## Deliverables
- `decay_if_needed(expr) -> HirExpr`.
- Fixture tests for every exception.

## Acceptance
- `int arr[10]; int *p = arr;` inserts `ArrayToPtr` around `arr`.
- `int arr[10]; sizeof arr;` does NOT decay — `sizeof` returns 40.

## References
- C99 §6.3.2.1.
