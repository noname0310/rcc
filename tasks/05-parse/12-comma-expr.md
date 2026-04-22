# 05-12: Comma operator

**Phase:** 05-parse    **Depends on:** 05-08    **Milestone:** M1+

## Goal
Parse `a, b` as a left-associative binary operator with the lowest
precedence. Argument lists parse without the comma operator visible
(each argument is an `assignment-expression` per §6.5.17p2).

## Scope
- In: `parse_expression()` wraps `parse_expr_bp(0)` and folds `,`;
  `parse_assignment_expression()` stops before `,`; function-call
  arg parser uses the latter.
- Out: --.

## Deliverables
- Two parser entry points with their contract documented in the
  module.
- Tests: `a, b, c` (three-level Comma), `f(a, b)` (two args, no
  nested Comma).

## Acceptance
- `f(a, b)` yields a `Call { callee, args: [a, b] }`, **not** a
  `Call { callee, args: [Comma(a,b)] }`.

## References
- C99 §6.5.17, §6.5.2.2.
