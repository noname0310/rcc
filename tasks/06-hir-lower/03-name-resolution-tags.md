# 06-03: Tag namespace resolution

**Phase:** 06-hir-lower    **Depends on:** 06-01    **Milestone:** M4

## Goal
C99 has a **separate** namespace for struct/union/enum tags. Resolve
`struct S`, `union U`, `enum E` references to the appropriate `DefId`
(or error on mismatch of kind, E0072).

## Scope
- In: `Resolver::tags` keyed by `Symbol`; distinguish `RecordKind` vs
  `Enum` entry; forward declarations (`struct S;`) allowed.
- Out: completing incomplete tags at a later point in the TU.

## Deliverables
- Resolution logic in type-spec lowering.
- Tests: mutual recursion (`struct A; struct B { A *a; };`),
  kind mismatch (`struct S` then `union S`).

## Acceptance
- Mutually recursive structs resolve.
- `struct S { int x; }; union S;` → E0072.

## References
- C99 §6.2.3.
