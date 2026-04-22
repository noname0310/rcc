# 04-14: Conditional-compilation state machine

**Phase:** 04-preprocess    **Depends on:** 04-13    **Milestone:** M5

## Goal
Track nested `#if` / `#ifdef` / `#ifndef` / `#elif` / `#else` /
`#endif` with a stack so inactive regions skip token output and
inner directives still participate in the state tracking.

## Scope
- In: stack of `CondFrame { taken: bool, active: bool, else_seen: bool }`;
  transition rules per C99 §6.10.1; unmatched `#else` / `#endif`
  = E0028 / E0029.
- Out: expression evaluation (that's task 13).

## Deliverables
- `CondStack` type + reduction on each directive.
- Tests: deeply nested (`4+` levels), duplicate `#else` error,
  missing `#endif` at EOF.

## Acceptance
- Directives inside an inactive branch still decrement the nesting
  on `#endif` (i.e. the stack remains consistent).
- Unterminated `#if` at EOF produces E0030 with a label on the
  `#if` keyword.

## References
- C99 §6.10.1.
