# 11-16c: tcc-tests2 typedef function declarators

**Phase:** 11-conformance    **Depends on:** 11-16    **Milestone:** M6

## Goal
Fix the parser/HIR lowering path that rejects legal typedef-based function
declarators.

## Scope
- In: `tcc-tests2::39_typedef`.
- Out: K&R-only extensions unrelated to the failing typedef form.

## Deliverables
- A parser regression around line 62 of `39_typedef.c`.
- HIR lowering/type-name checks proving typedef-name classification remains
  correct in parameter and function-declarator contexts.

## Acceptance
- `39_typedef` compiles and passes through tcc-tests2.
- The diagnostic `expected K&R parameter declaration or function body` is not
  emitted for a legal prototype/function declarator.

## References
- `target/wsl/tcc-tests2-16-final.json`
- C99 §6.7.5.3.
