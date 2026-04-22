# 04-13: `#if` expression evaluator

**Phase:** 04-preprocess    **Depends on:** 04-08    **Milestone:** M5

## Goal
Evaluate `#if` / `#elif` controlling expressions per C99 §6.10.1. The
preprocessor has its **own** integer-only expression evaluator (no
floats, no casts). Identifiers that are not macros are replaced with
`0` before the expression is parsed.

## Scope
- In: `defined X` / `defined(X)` handled before macro expansion;
  post-expansion, remaining identifiers → `0`; expression parser
  supporting `+`, `-`, `*`, `/`, `%`, `<<`, `>>`, `<`, `<=`, `>`,
  `>=`, `==`, `!=`, `&`, `^`, `|`, `&&`, `||`, `? :`, `!`, `~`;
  integer arithmetic done in `i128` / `u128` based on suffix.
- Out: float / pointer in `#if` (explicitly forbidden by standard).

## Deliverables
- `eval_if(tokens: &[PpToken], macros: &MacroTable) -> Result<i128, Diagnostic>`.
- Tests: `#if 1+1 == 2`, `#if defined FOO`, `#if !defined BAR`.

## Acceptance
- chibicc's `test/macro.c` `#if` tests pass.
- Division by zero in a live `#if` branch emits E0027.

## References
- C99 §6.10.1.
