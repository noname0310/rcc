# 06-06: Declarator → `Ty`

**Phase:** 06-hir-lower    **Depends on:** 06-05    **Milestone:** M2

## Goal
Fold a parsed `Declarator` (name + chain of `DerivedDeclarator`) over
a base `Ty` obtained from `DeclSpecs`. Produces the final `Ty` for the
declared name; applies qualifiers correctly.

## Scope
- In: iterate chain outside-in; handle pointer quals; array size
  resolution (constant-expr via `rcc_typeck::ConstEval` stub);
  function-type construction; reject (with E0076) illegal forms like
  `void x;` for an object or `int f()[10]` (function returning array).
- Out: VLA array size deferred — handled in the CFG phase.

## Deliverables
- `apply_declarator(base: TyId, d: &Declarator, tcx) -> TyId`.
- Golden table covering §6.7.5 examples.

## Acceptance
- `int (*fp[3])(int)` → `Array[3] of Ptr to Func(int)->int`.
- `int arr[]` at function scope → error; at file scope → incomplete
  type with len `None`.

## References
- C99 §6.7.5.
