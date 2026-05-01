# 09-09: Place address, operand load, and store helpers

**Phase:** 09-codegen-llvm    **Depends on:** 09-05, 09-06    **Milestone:** M3

## Goal

Centralize the memory model for CFG lowering: address a `Place`, load an
`Operand`, store into a `Place`, and emit `Rvalue::AddressOf`.

## Scope

- In: base locals/globals, `Projection::Deref`, `Projection::Field`,
  `Projection::Index`, `Operand::Copy`, `Operand::Move`, constants, and aggregate
  vs scalar distinction.
- Out: bitfield read/write; owned by 09-21.

## Deliverables

- `emit_place_addr`, `emit_operand_value`, `emit_store_place`.
- Tests covering `*p`, `s.f`, `a[i]`, nested projections, and address-of.

## Acceptance

- Later rvalue/terminator tasks do not perform ad-hoc GEP or load/store logic.
- Invalid place/type combinations become backend errors, not panics.

## References

- `rcc_cfg::Place`
- LLVM LangRef: `getelementptr`
