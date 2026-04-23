> ✓ done — 2026-04-23

# 04-06: Object-like `#define`

**Phase:** 04-preprocess    **Depends on:** 04-02    **Milestone:** M1+

## Goal
Parse object-like `#define NAME replacement-list` into a
`MacroDef { kind: ObjectLike, ... }`. Redefinition with a different
body is E0022.

## Scope
- In: store definition in `MacroTable`; compare redefinitions by
  token-equivalence per C99 §6.10.3p1; handle `#undef` to remove.
- Out: expansion (task 08).

## Deliverables
- `define_object_like(name, body, span, macros: &mut MacroTable)`.
- Tests: redefinition identical (ok), differing (E0022), `#undef`
  then redefine (ok).

## Acceptance
- Round-trip: define `FOO 42`, look up, body is the `42` pp-token.
- E0022 diagnostic has labels on both definitions.

## References
- C99 §6.10.3.
