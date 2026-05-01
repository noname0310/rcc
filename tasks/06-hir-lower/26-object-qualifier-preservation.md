# 06-26: Preserve object qualifiers in HIR

> ✓ done — 2026-05-01

**Phase:** 06-hir-lower    **Depends on:** 06-25    **Milestone:** M3 pre-codegen stabilization

## Goal
Preserve top-level `const` / `volatile` qualifiers on declared
objects, parameters, globals, and fields. Codegen cannot emit
`volatile` loads/stores, and typeck cannot reject writes to `const`
objects, if HIR only keeps qualifiers on pointer pointees or array
elements.

## Scope
- In: represent declaration-level qualifiers for file-scope globals,
  block-scope locals, parameters, and record fields.
- In: make `lower_specs_to_base_ty_in_scope` or its caller apply
  `DeclSpecs.quals` without corrupting typedef and derived-declarator
  semantics.
- In: keep pointer/array element qualifiers working exactly as before.
- Out: LLVM volatile instruction emission; owned by 09-18.
- Out: `restrict` alias analysis metadata; owned by a later quality /
  optimization task.

## Deliverables
- HIR data model extension for object qualifiers, or an equivalent
  side table keyed by `DefId` / `Local` / field index.
- HIR lowering tests for:
  - `volatile int g;`
  - `const int local = 1;`
  - `struct S { volatile int x; };`
  - `int * const p;` vs `const int *p;`
- Pretty/debug output or test helper that exposes qualifier metadata.

## Acceptance
- `volatile int x;` reaches HIR with a volatile-qualified object.
- `const int x;` reaches HIR with a const-qualified object.
- `const int *p;` still means pointer to const int, while `int * const p;`
  means const-qualified pointer object.
- Existing pointer-conversion tests that rely on `Qual` still pass.

## References
- C99 §6.7.3 — type qualifiers.
- `tasks/09-codegen-llvm/20-volatile-access.md`.
- Follow-up: 07-20 enforces qualifier-sensitive type rules.
