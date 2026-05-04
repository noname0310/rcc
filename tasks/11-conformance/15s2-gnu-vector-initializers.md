> ✓ done — 2026-05-04

# 11-15s2: GNU vector initializers and compound literals

**Phase:** 11-conformance    **Depends on:** 11-15s1    **Milestone:** M6

## Goal
Lower vector initializer lists and vector compound literals without treating
vectors as records.

## Scope
- In: `{ 1, 2, 3, 4 }` vector initializers.
- In: `(v4si){ 1, 2, 3, 4 }` compound literals.
- In: zero-fill for omitted lanes.
- Out: vector arithmetic and ABI.

## Deliverables
- HIR/global-init representation for vector constants.
- CFG lowering for vector local/global initializers.
- LLVM constant/vector construction tests.
- Reduced fixture from `20050604-1`.

## Acceptance
- A vector object initialized with a lane list has the expected byte view.
- Compound literal vectors can be assigned to vector variables.
- No initializer path reuses aggregate record field indexing for vectors.

## References
- `docs/gnu-vector-design.md`
- `gcc-torture::execute::20050604-1`
