> ??done ??2026-05-03

# 09-14: Binary and unary op emission

**Phase:** 09-codegen-llvm    **Depends on:** 09-09, 09-12    **Milestone:** M3

## Goal

Map `rcc_cfg::BinOp` and `rcc_cfg::UnOp` to LLVM integer, floating, pointer,
and comparison instructions.

## Scope

- In: integer arithmetic, signed/unsigned div/rem, shifts, bitwise ops, integer
  and floating comparisons, floating arithmetic, pointer +/- integer, pointer
  difference, `Neg`, `FNeg`, `BitNot`, and `LogNot`.
- Out: complex arithmetic; owned by 09-18.

## Deliverables

- Exhaustive match coverage for `BinOp` and `UnOp`.
- Table-driven tests for instruction opcode and result type.

## Acceptance

- Adding a new CFG op fails compilation until codegen handles it.
- Pointer arithmetic uses element layout, not raw byte increments.

## References

- `rcc_cfg::BinOp`
- `rcc_cfg::UnOp`
