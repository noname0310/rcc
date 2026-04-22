# 08-04: Place projections

**Phase:** 08-cfg    **Depends on:** 08-03    **Milestone:** M3

## Goal
Compose `Projection::Deref | Field | Index` to represent any lvalue.
`a[i].b->c` lowers to a single `Place` with four projection steps.

## Scope
- In: projection composition; `Index` consumes an `Operand` (not a
  `Place`) — integer-valued lvalues are first dereffed into a temp.
- Out: VLA-indexed places (task 13).

## Deliverables
- Lowering helpers.
- Fixtures: `struct { int a[3]; } s; s.a[2]` → `Place{base: s, proj: [Field(0), Index(2)]}`.

## Acceptance
- Chained lvalue expressions produce a single `Place`, not a chain of
  temporaries.

## References
- rustc MIR `Place` model.
