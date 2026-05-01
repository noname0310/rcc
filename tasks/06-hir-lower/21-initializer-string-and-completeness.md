# 06-21: initializer completeness, strings, and diagnostics

**Phase:** 06-hir-lower    **Depends on:** 06-20    **Milestone:** M5 stabilization

## Goal
Make initializer lowering preserve enough semantics for typeck/CFG and
avoid silently dropping malformed or special initializer forms.

## Scope
- In: string literal initialization of `char[]`, `char[N]`, and
  compatible qualified char arrays.
- In: incomplete array completion from initializer length.
- In: explicit diagnostics or structured error nodes for excess
  initializers and bad designators.
- In: static/global initializer representation needed by later codegen.
- Out: full constant-data emission; that remains codegen work.

## Deliverables
- Array length completion for `int a[] = {1,2,3}` and
  `char s[] = "hi"`.
- String literal initializer lowering without routing through pointer
  decay.
- Tests for array, record, union, nested, and designated initializers.

## Acceptance
- `char s[] = "hi";` lowers as an array of length 3 including `\0`.
- `int a[] = {1,2,3};` has known length 3 after HIR lowering.
- `{ [4] = 1 }` completes an incomplete array to length 5.
- Bad designators are retained or diagnosed; they are not silently
  skipped without a test.

## References
- C99 §6.7.8 — Initialization.
- `lower_initializer` currently documents string initialization and
  some diagnostics as deferred.

