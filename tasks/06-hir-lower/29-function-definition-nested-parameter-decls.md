> ✓ done — 2026-05-04

# 06-29: nested function-declarator parameter lowering

**Phase:** 06-hir-lower    **Depends on:** 06-27    **Milestone:** M6+

## Goal
Collect function-definition parameters from the declarator level that owns the
function body, even when the function returns a pointer to a function.

## Trigger
- `c-testsuite::00124` defines
  `int (*f1(int a, int b))(int c, int b)` but the body cannot resolve
  parameter `a`.

## Scope
- In:
  - Identify the correct `DerivedDeclarator::Function` attached to the
    function definition name.
  - Lower only that parameter list as function-body locals.
  - Do not confuse nested function-pointer return parameter names with body
    parameters.
  - Preserve prototype type lowering for returned function pointers.
- Out:
  - K&R definition compatibility.

## Deliverables
- HIR-lower tests for functions returning function pointers and for nested
  function-pointer parameter names.
- c-testsuite regression for `00124`.

## Acceptance
- `a` and `b` in `f1` resolve to function parameters.
- `c-testsuite::00124` passes.

## References
- C99 §6.7.5.3
- `third_party/testsuites/c-testsuite/tests/single-exec/00124.c`
