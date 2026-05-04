# 11-15q: gcc-torture aggregate memory cluster

> ✓ done — 2026-05-04 — fixed `__builtin_offsetof` lowering and GNU
> byte-order predefined macros; WSL gcc-torture aggregate cluster is 3/3 pass.

**Phase:** 11-conformance    **Depends on:** 11-15k    **Milestone:** M6

## Goal
Reduce and fix remaining aggregate, pointer, and byte-layout runtime bugs.

## Scope
- In: `pr37573`, `pr49390`, `pr65401`.
- Out: bit-field-only aggregate cases already covered by `11-15l`.

## Deliverables
- Reductions for char-byte views of aggregate storage, nested aggregate copies,
  and struct by-value ABI interactions.
- Codegen or CFG fixes with runtime tests.

## Acceptance
- Every listed case either passes or has a narrower task with a reduced fixture.
- No result masking is added.

## References
- `docs/gcc-torture-signal-clusters.md`
