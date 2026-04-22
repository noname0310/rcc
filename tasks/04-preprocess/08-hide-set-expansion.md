# 04-08: Hide-set macro expansion (Prosser)

**Phase:** 04-preprocess    **Depends on:** 04-06, 04-07    **Milestone:** M5

## Goal
Implement Dave Prosser's algorithm (the closest public match to the
standard's expansion rules) for both object-like and function-like
macros. The hide-set prevents self / mutual recursion and matches the
expected behaviour of every major compiler.

## Scope
- In: `expand(tokens) -> expanded_tokens`; per-token `HideSet` carried
  through; function-like call argument collection respects nested
  parentheses; recursion detection purely through hide-set membership.
- Out: `#`/`##` operators (tasks 09/10); variadic `__VA_ARGS__`
  (task 11).

## Deliverables
- `Preprocessor::expand_one(token) -> Vec<PpToken>`.
- Reference-implementation tests derived from chibicc's
  `test/macro.c` non-variadic subset.

## Acceptance
- `#define FOO FOO` expands to the literal identifier `FOO` (hide-set
  blocks recursion).
- `#define A B / #define B A / A`: expansion terminates with `A`
  emitted.
- Function-like call with embedded commas in parentheses collects
  one argument, not two.

## References
- Dave Prosser paper: "Standard Algorithms for the C Preprocessor".
