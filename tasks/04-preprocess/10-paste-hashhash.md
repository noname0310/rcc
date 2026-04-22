# 04-10: `a ## b` token-paste operator

**Phase:** 04-preprocess    **Depends on:** 04-08    **Milestone:** M5

## Goal
Implement the `##` operator per C99 §6.10.3.3. Concatenate the left
and right operand texts, lex the result as a single pp-token; if the
lex produces multiple tokens, emit E0025 ("pasting forms an invalid
token").

## Scope
- In: paste happens before the enclosing hide-set expansion; the
  resulting token's hide-set is the intersection of the operands'
  hide-sets; empty argument paste rules (variadic corner case).
- Out: variadic-specific paste handling (task 11).

## Deliverables
- `paste(lhs: PpToken, rhs: PpToken) -> Result<PpToken, Diagnostic>`.
- Tests: identifier paste (`a##b` → `ab`), number paste
  (`1##2` → `12`), invalid (`+##;` → E0025).

## Acceptance
- Classical `#define CAT(a,b) a##b / CAT(lo,oo)p` expands to `loop`.
- Invalid paste produces E0025 with both operand spans labelled.

## References
- C99 §6.10.3.3.
