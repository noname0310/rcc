# 07-24: casted string global initializer

> ✓ done — 2026-05-05

**Phase:** 07-typeck    **Depends on:** 07-16, 07-19    **Milestone:** M6+

## Goal

Fold casted string literals in file-scope pointer initializers as address
constants.

## Trigger

zlib `zutil.c` defines:

```c
z_const char * const z_errmsg[10] = {
    (z_const char *)"need dictionary",
    ...
};
```

HIR lowering correctly materializes each string literal as a synthetic global,
and typeck accepts the expression as a constant initializer. The value folding
step still left every leaf as `GlobalInitValue::Error` because
`ConstEval::eval_address` recognized `DefRef` globals but not `StringRef`
globals.

## Scope

- In:
  - Treat `StringRef(def)` as an address constant `(def, 0)`.
  - Preserve existing casts and array-to-pointer conversions around the literal.
  - Add a const-eval regression test for `(char *)"..."`.
- Out:
  - Codegen materialization of the string literal payload.
  - General pointer casts from nonzero integer constants.
  - Writable string literal semantics.

## Acceptance

- [x] `ConstEval::eval_address((char *)"...")` returns the string literal def.
- [x] `ConstEval::eval_scalar((char *)"...")` returns `ConstScalar::Address`.
- [x] zlib `zutil.c --emit=hir` no longer reports initializer error leaves for `z_errmsg`.
- [x] zlib smoke progresses past the previous `zutil.c` global initializer blocker.

## References

- C99 §6.4.5p5-p6 string literal storage duration
- C99 §6.6p9 address constants
- `real_world/projects/03-zlib/plan.md`
