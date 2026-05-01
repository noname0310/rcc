> ✓ done — 2026-05-01

# 07-17: Control-expression and conditional-operator constraints

**Phase:** 07-typeck    **Depends on:** 07-16    **Milestone:** M3 pre-codegen stabilization

## Goal
Validate scalar controlling expressions and the full `?:` type rules
before CFG lowers them into branches. CFG should not need to guess
whether a condition is scalar or whether a conditional result type is
valid.

## Scope
- In: emit diagnostics when `if`, `while`, `do`, `for`, `switch`, `&&`,
  `||`, or the first operand of `?:` uses a non-scalar expression.
- In: implement C99 conditional-operator result typing for arithmetic,
  compatible pointers, null pointer constants, `void`, and compatible
  struct/union operands.
- In: insert required conversions into the selected arms.
- Out: GNU omitted-middle `?:` extension.

## Deliverables
- Helper for "scalar controlling expression" validation.
- Type unification helper for `?:`.
- Tests for arithmetic, pointer/null, `void`, aggregate, and invalid
  condition cases.

## Acceptance
- `if ((struct S){0}) {}` emits a scalar-condition diagnostic.
- `p ? p : 0` yields the pointer type and inserts the needed null
  pointer conversion.
- `cond ? (void)f() : (void)g()` yields `void`.
- Incompatible pointer arms emit a diagnostic instead of choosing the
  then-arm as a placeholder.

## References
- C99 §6.5.13, §6.5.14, §6.5.15, §6.8.4.
- `crates/rcc_typeck/src/lib.rs`, current `scalar_rvalue` and
  conditional placeholder paths.
