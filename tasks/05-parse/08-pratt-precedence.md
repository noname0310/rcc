# 05-08: Pratt precedence parser

**Phase:** 05-parse    **Depends on:** 05-07    **Milestone:** M1+

## Goal
Parse binary expressions with a Pratt loop driven by the C99 §6.5
precedence table. Supports every C99 binary + assignment operator.

## Scope
- In: table `infix_bp(op) -> Option<(u8, u8)>` (left/right binding);
  associativity: assignments are right-associative, everything else
  left; hand-roll comma / ternary as special cases (tasks 11, 12).
- Out: unary/postfix (task 09), cast/sizeof (task 10).

## Deliverables
- `parse_expr_bp(min_bp: u8) -> Expr`.
- Tests for all operators; deep nesting (≥ 32 levels) without
  stack overflow.

## Acceptance
- `a + b * c` parses as `a + (b*c)`.
- `a = b = c` parses as `a = (b = c)`.
- `a == b != c` parses as `(a==b) != c`.

## References
- C99 §6.5 precedence.
- Matklad's "Simple but Powerful Pratt Parsing" writeup.
