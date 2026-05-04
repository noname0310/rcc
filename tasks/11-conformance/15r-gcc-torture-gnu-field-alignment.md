# 11-15r: gcc-torture GNU field alignment

**Phase:** 11-conformance    **Depends on:** 11-15k    **Milestone:** M6

## Goal
Extend GNU `aligned(N)` support from record-level alignment to field/member
alignment.

## Scope
- In: `pr23467`, field-level `__attribute__((aligned(N)))`.
- Out: `packed`, `section`, and target-specific attributes.

## Deliverables
- AST/HIR metadata for field alignment overrides.
- Layout service support for raising member offsets and enclosing record
  alignment.
- LLVM type/global constant tests and WSL runtime probe.

## Acceptance
- `gcc-torture::execute::pr23467` passes with `-fgnu-attributes`.
- Record-level alignment tests from `11-15i` still pass.

## References
- `docs/gcc-torture-signal-clusters.md`
