> ✓ done — 2026-05-04

# 06-28: block-scope tag shadowing

**Phase:** 06-hir-lower    **Depends on:** 06-18    **Milestone:** M6+

## Goal
Give struct/union/enum tags proper C block scope instead of using one
translation-unit-wide tag map.

## Trigger
- `c-testsuite::00044` rejects a block-local `struct T` that should shadow a
  file-scope `struct T`.
- `c-testsuite::00053` reuses `struct T` in an inner block and then reports
  cascading wrong-field diagnostics.

## Scope
- In:
  - Replace or wrap `Resolver::tags` with a scoped tag stack for function body
    lowering.
  - Preserve file-scope tags and forward declarations across the translation
    unit.
  - Allow an inner block to introduce a new tag with the same spelling.
  - Ensure lookups find the nearest enclosing tag scope.
  - Keep duplicate tag definitions in the same scope diagnosed.
- Out:
  - Anonymous struct/union member extension semantics.

## Deliverables
- HIR-lower unit tests for file-scope tag reuse, block-scope shadowing, and
  duplicate same-scope tag definitions.
- c-testsuite regression for `00044` and `00053`.

## Acceptance
- `c-testsuite::00044` and `c-testsuite::00053` compile and execute
  successfully.
- Existing tag completion tests still pass.

## References
- C99 §6.2.1, §6.2.3, §6.7.2.3
- `crates/rcc_hir_lower/src/lib.rs`
