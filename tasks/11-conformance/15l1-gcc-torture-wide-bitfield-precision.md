# 11-15l1: gcc-torture wide bit-field precision

**Phase:** 11-conformance    **Depends on:** 11-15l    **Milestone:** M6

## Goal
Fix the remaining bit-field runtime aborts that require GNU/GCC-compatible
precision for bit-fields wider than `int` but narrower than their declared
storage type.

## Scope
- In: `bitfld-3`, `bitfld-5`, `pr32244-1`, `pr34971`.
- In: arithmetic, shifts, increment/decrement, and casts involving
  `unsigned long long : 33..40` style bit-fields.
- Out: `scalar_storage_order`, vector bit-fields, and inline asm semantics.

## Deliverables
- A checked representation for "bit-field value precision" that does not
  corrupt CFG place/storage types.
- Codegen or CFG masking/truncation at the operation boundary when GCC expects
  arithmetic to wrap to the bit-field precision rather than the declared
  `unsigned long long` width.
- Reduced runtime fixtures for:
  - wide bit-field multiply/add/sub wrap (`bitfld-3`)
  - wide bit-field shift precision (`pr32244-1`)
  - wide bit-field rotate expression (`pr34971`)
  - wide bit-field cast/adjust path (`bitfld-5`)

## Acceptance
- The four scoped gcc-torture cases pass under WSL LLVM:
  `bitfld-3`, `bitfld-5`, `pr32244-1`, `pr34971`.
- No xfail, skip, or result masking is added.
- Storage layout remains unchanged for the 15j/15l cases already fixed.

## References
- `tasks/11-conformance/15l-gcc-torture-bitfield-precision-cluster.md`
- `docs/gcc-torture-signal-clusters.md`
