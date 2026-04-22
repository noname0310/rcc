# 06-05: Typedef expansion

**Phase:** 06-hir-lower    **Depends on:** 06-01    **Milestone:** M2

## Goal
When building a `Ty` from a `DeclSpecs` that contains
`TypeSpec::TypedefName(sym)`, look up the typedef's `DefKind::Typedef(TyId)`
and inline it. The resulting `Ty` does **not** remember that the
typedef was ever involved — ideal for `TyCtxt` dedup.

## Scope
- In: resolution + inline; cycle detection (typedef referring to
  itself through another typedef) → E0075.
- Out: pretty-printing that preserves the typedef name for
  diagnostics (future polish).

## Deliverables
- `lower_typedef_name(sym, tcx) -> TyId`.
- Tests including a typedef chain.

## Acceptance
- `typedef int T; typedef T U; U x;`: `x`'s type is
  `TyCtxt::int` (interned singleton), not a new type.

## References
- C99 §6.7.7.
