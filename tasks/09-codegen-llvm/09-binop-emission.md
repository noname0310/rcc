# 09-09: Binary op emission

**Phase:** 09-codegen-llvm    **Depends on:** 09-08    **Milestone:** M3

## Goal
Map every `rcc_cfg::BinOp` to the right LLVM instruction. Most map
1:1 (`Add` → `add` on integers, `FAdd` → `fadd`) but pointer arithmetic
uses `getelementptr`.

## Scope
- In: table-driven match; `PtrAdd` / `PtrSub` → `getelementptr` with
  correct inbounds flag; `PtrDiff` → `ptrtoint` subtract divide.
- Out: overflow semantics ornamentation (`nsw`, `nuw`) — apply per
  C99 signedness.

## Deliverables
- `emit_binop(op, lhs, rhs) -> LLValue`.
- FileCheck test per op (task 16 aggregates).

## Acceptance
- `int c = a + b;` emits `add nsw i32`.
- `unsigned c = a + b;` emits plain `add i32`.

## References
- LLVM LangRef binary operators.
