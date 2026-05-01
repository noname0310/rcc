# 06-17: C declarator scope timing

**Phase:** 06-hir-lower    **Depends on:** 06-16    **Milestone:** M5 stabilization

## Goal
Align declaration lowering with C's rule that an identifier's scope
begins just after the completion of its declarator.

## Scope
- In: block locals, block typedefs, file-scope objects, file-scope
  typedefs, and multi-declarator declarations.
- In: initializer expression lookup for `int x = x;`.
- In: array-bound expressions and VLA length expressions.
- Out: uninitialized-value analysis.

## Deliverables
- Lowering order is explicit: compute declarator type, register the new
  binding at the correct point, then lower initializer expressions with
  the right visible identifiers.
- Regression tests for shadowing and self-reference cases.

## Acceptance
- `int x = 1; void f(void) { int x = x; }` resolves initializer `x` to
  the inner declaration according to C scope timing, even if later
  semantic analysis warns about indeterminate value.
- `void f(void) { int a = 1, b = a; }` resolves `b`'s initializer to
  the earlier `a`.
- `void f(void) { int a = sizeof a; }` sees `a` as the object being
  declared.
- A later declarator in `int a, a;` gets a duplicate declaration
  diagnostic rather than silently overwriting the binding.

## References
- C99 §6.2.1p7 — identifier scope begins just after its declarator.
- `lower_block_decl` currently lowers scalar initializers before
  inserting the local binding.

