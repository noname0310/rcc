# 07-16: Global initializer const-eval pipeline

**Phase:** 07-typeck    **Depends on:** 07-15    **Milestone:** M4 pre-global-codegen

## Goal
Make file-scope initializer values codegen-ready. Legal C99 static
initializers such as `2 + 3`, address constants, casts, and nested
aggregate leaves must be folded into `GlobalInitValue` instead of
collapsing to `GlobalInitValue::Error`.

## Scope
- In: preserve enough initializer expression information from HIR lower,
  or re-lower static initializer leaves into temporary HIR expressions
  that `ConstEval` can evaluate.
- In: wire `check_init_const` into the real crate-level `check()` path.
- In: update `DefKind::Global { init }` after const evaluation.
- In: report E0084 for non-constant static initializer leaves.
- Out: relocation emission details; owned by 09-11.

## Deliverables
- Typeck pass over `DefKind::Global` initializers.
- Const-evaluated `GlobalInitValue` for integer, floating, string, and
  address-constant leaves.
- Regression tests for scalar, array, record, and string initializers.

## Acceptance
- `static int x = 2 + 3;` stores `GlobalInitValue::Int(5)`.
- `static int *p = &x;` stores an address-constant representation that
  09-11 can turn into an LLVM initializer.
- `static int y = f();` emits E0084 and blocks CFG/codegen.
- Existing aggregate designator/range designator lowering remains
  deterministic.

## References
- C99 §6.6 and §6.7.8p4.
- `crates/rcc_typeck/src/init_const.rs`.
- `crates/rcc_hir_lower/src/lib.rs`, current `GlobalInitValue::Error`
  fallback for non-literal initializer expressions.
