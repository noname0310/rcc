> ✓ done — 2026-05-04

# 11-16c: tcc-tests2 typedef function declarators

**Phase:** 11-conformance    **Depends on:** 11-16    **Milestone:** M6

## Goal
Fix the parser/HIR lowering path that rejects typedef-based and GNU
`typeof`-based function declarators in the `39_typedef` fixture.

## Scope
- In: `tcc-tests2::39_typedef`.
- Out: K&R-only extensions unrelated to the failing typedef / `typeof` form.

## Deliverables
- Parser regressions around GNU `typeof` declaration specifiers.
- HIR lowering/type-name checks proving typedef-name classification remains
  correct in parameter and function-declarator contexts.

## Acceptance
- `39_typedef` compiles and passes through tcc-tests2.
- The diagnostic `expected K&R parameter declaration or function body` is not
  emitted for a legal prototype/function declarator.

## References
- `target/wsl/tcc-tests2-16c-typedef.json`
- `target/wsl/tcc-tests2-16c-full.json`
- C99 §6.7.5.3.
