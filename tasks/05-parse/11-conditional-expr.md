# 05-11: Conditional expression

**Phase:** 05-parse    **Depends on:** 05-08    **Milestone:** M1+

## Goal
Parse `a ? b : c` with right-associativity and correct precedence
(just above assignment; below `||`).

## Scope
- In: special handling inside the Pratt loop when `?` is seen; after
  parsing the *then* expression, expect `:` (else E0050).
- Out: `? :` with missing middle operand (GCC extension; not C99).

## Deliverables
- Branch inside `parse_expr_bp`.
- Tests: `a ? b : c`, `a ? b ? c : d : e`, `a = b ? c : d`.

## Acceptance
- Associativity verified: `a ? b : c ? d : e` → `a ? b : (c ? d : e)`.
- Missing `:` → E0050 with a label on the `?` token.

## References
- C99 §6.5.15.
