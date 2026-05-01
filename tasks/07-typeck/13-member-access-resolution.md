# 07-13: Resolve member access types and field indices

**Phase:** 07-typeck    **Depends on:** 06-25    **Milestone:** M3 pre-codegen stabilization

## Goal
Resolve `a.b` and `a->b` against the base record type during typeck.
After this task, CFG receives only resolved `Field { field_index }`
expressions with the correct result type and value category.

## Scope
- In: look through lvalue/rvalue wrappers as needed to identify the
  record or pointer-to-record base type.
- In: resolve named fields by name, including union members.
- In: set the member expression's `ty` to the selected field type and
  its value category according to C99 §6.5.2.3.
- In: emit a diagnostic for unknown members, non-record bases, and
  `->` on non-pointer bases.
- Out: bitfield load/store codegen; owned by 09-19.

## Deliverables
- Typeck resolver for preserved member-access names from 06-25.
- E-code-backed diagnostics for invalid member access.
- Removal of the temporary CFG compatibility fallback that maps unresolved
  member accesses to `Projection::Field(0)`.
- Tests covering `s.a`, `s.b`, `p->b`, union members, missing members,
  and non-record base expressions.

## Acceptance
- `struct S { int a; long b; }; long f(struct S s) { return s.b; }`
  typechecks with the member expression typed as `long`.
- `struct S { int a; int b; }; int f(struct S s) { return s.b; }`
  lowers to CFG with `Projection::Field(1)`.
- `int x; x.y;` emits a typeck diagnostic and does not reach CFG as a
  valid field projection.
- `rcc_cfg::lower` has no fallback arm that accepts
  `HirExprKind::UnresolvedField` as a valid projection.

## References
- C99 §6.5.2.3.
- 06-25.
- `crates/rcc_typeck/src/lib.rs`, current placeholder-preserving field arm.
