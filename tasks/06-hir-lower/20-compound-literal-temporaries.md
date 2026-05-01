> ✓ done — 2026-05-01

# 06-20: compound literal temporary objects

**Phase:** 06-hir-lower    **Depends on:** 06-19    **Milestone:** M5 stabilization

## Goal
Lower C99 compound literals into real HIR storage instead of
`IntConst(0)` placeholders.

## Scope
- In: block-scope compound literals as synthetic locals with automatic
  storage duration.
- In: scalar, array, record, and typedef-named compound literal types.
- In: brace initializer reuse through `lower_initializer`.
- Out: file-scope compound literals as synthetic internal globals; this
  is handled with global/static initializer representation in 06-21.
- Out: non-standard GNU compound literal extensions.

## Deliverables
- A HIR representation for the compound literal lvalue.
- Synthetic local naming that is stable in tests.
- Initializer lowering wired to the synthetic object.
- Typeck and CFG support for the produced shape.

## Acceptance
- `int *p = &(int){3};` creates a temporary lvalue object.
- `return ((struct S){ .x = 1 }).x;` lowers through record initializer
  machinery.
- `(int[3]){1,2,3}[1]` is indexable as an array lvalue.
- The old `expr_compound_literal_placeholder` test is replaced.

## References
- C99 §6.5.2.5 — Compound literals.
- `lower_expr` currently placeholder-lowers `CompoundLiteral`.

## Plan-level concern (agent)

**Detected:** 2026-05-01

This task currently includes both:

- block-scope compound literals as synthetic automatic locals; and
- file-scope compound literals as synthetic internal globals for constant
  initialization.

The first item fits the current HIR/CFG shape. The second item depends
on a representation for static/global initializers, but that
representation is explicitly scheduled in `06-21`:

> `06-21` scope: "static/global initializer representation needed by
> later codegen."

Current `DefKind::Global` stores only `{ ty, linkage }`, so there is no
place to attach either the original initializer, lowered initializer
leaves, or a synthetic global's initializer payload. Implementing the
file-scope half inside `06-20` would either duplicate `06-21` or force a
larger HIR global-initializer design ahead of the current task order.

Suggested reshape:

- Keep `06-20` focused on block-scope compound literals: synthetic HIR
  locals, initializer-lowering reuse, typeck value category, and CFG
  storage/lvalue support.
- Move file-scope compound literals to `06-21`, where global/static
  initializer representation is already in scope.

Question for user: should `06-20` be reshaped this way, or should
global initializer representation be pulled forward into `06-20`?

**Decision:** user approved the reshape on 2026-05-01. `06-20` is now
block-scope only; file-scope compound literals move to `06-21`.
