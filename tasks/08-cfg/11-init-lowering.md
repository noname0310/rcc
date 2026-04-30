> ✓ done — 2026-05-01

# 08-11: Aggregate initializer lowering

**Phase:** 08-cfg    **Depends on:** 08-04, 06-11    **Milestone:** M4

## Goal
Lower HIR-level initializer plans into CFG: zero-fill the target
slot with a `memset` intrinsic, then emit `Assign` per leaf value
at the computed projection path.

## Scope
- In: emit `Statement::Assign { place, rvalue: Rvalue::Use(Operand::Const(ConstKind::ZeroInit)) }`
  first when the init is not fully dense; then the per-leaf stores.
- Out: global (static) initializers → `Const` value flattening is
  codegen's job.

## Deliverables
- `lower_init(place, init)` helper.
- Snapshot: `int a[5] = {1, 2}`.

## Acceptance
- `int a[1000] = {0};` produces one zero-fill + zero leaf stores.
- Designated `{[2] = 5}` produces zero-fill + single store at offset
  `2 * sizeof(int)`.

## References
- C99 §6.7.8.
