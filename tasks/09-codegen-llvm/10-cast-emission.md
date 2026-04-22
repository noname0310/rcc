# 09-10: Cast emission

**Phase:** 09-codegen-llvm    **Depends on:** 09-08    **Milestone:** M3

## Goal
Lower `Rvalue::Cast { kind, .. }` to the appropriate LLVM cast
instruction:
- `IntToInt`: `trunc` / `zext` / `sext`.
- `IntToFloat`: `sitofp` / `uitofp`.
- `FloatToInt`: `fptosi` / `fptoui`.
- `FloatToFloat`: `fptrunc` / `fpext`.
- `PtrToPtr`: `bitcast`.
- `IntToPtr` / `PtrToInt`: those instructions.

## Scope
- In: decide signedness from operand type; for integer narrowing,
  emit `trunc`.
- Out: `restrict` → `noalias` attribute (M7).

## Deliverables
- `emit_cast(op, to_ty, kind) -> LLValue`.

## Acceptance
- `(int)1.5f` emits `fptosi float 1.5 to i32`.
- `(unsigned char)(int)x` emits `trunc i32 to i8`.

## References
- LLVM LangRef conversion instructions.
