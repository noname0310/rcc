> ✓ done — 2026-05-04

# 06-32: string literal subaggregate initializer

**Phase:** 06-hir-lower    **Depends on:** 06-31    **Milestone:** M6+

## Goal
Keep C99 string-literal array initialization intact when the target char array
is a subobject of a larger aggregate.

## Trigger
- `c-testsuite::00204` currently fails before the aggregate-rvalue field-access
  path because declarations such as `struct s2 { char x[2]; } s2 = { "12" };`
  are lowered as scalar leaf assignments (`s2.x[0] = "12"`) and typeck reports
  `E0082`.

## Scope
- In:
  - Treat a string literal as initializing the whole current char/wide-char
    array subobject, even when it appears inside a surrounding brace list.
  - Preserve 06-31 flat scalar brace elision for genuine scalar lists.
  - Cover both local initializer statements and file-scope `GlobalInit` entries.
  - Preserve incomplete `char[]` completion for both `= "x"` and `= { "x" }`.
- Out:
  - Aggregate return ABI correctness in `00204`; that remains task 07-21 once
    this blocker is gone.

## Deliverables
- HIR-lower tests for local and global `struct { char x[N]; } = { "..." }`.
- Regression for `char s[] = { "hi" }` completing to length 3.
- Focused `00204` rerun to expose the next blocker.

## Acceptance
- `struct S { char x[3]; }; struct S s = { "abc" };` emits global init entries
  for `.x[0..2]` instead of a pointer-to-char assignment.
- `void f(void) { struct S s = { "abc" }; }` emits local stores to `s.x[i]`.
- `char s[] = { "hi" };` completes as `char[3]`.
- `c-testsuite::00204` no longer fails with `E0082` at its global string
  subaggregate initializers.
