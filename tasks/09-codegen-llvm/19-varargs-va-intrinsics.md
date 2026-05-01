# 09-19: Variadic function support

**Phase:** 09-codegen-llvm    **Depends on:** 09-06, 09-07, 09-08, 09-13    **Milestone:** M6

## Goal

Support variadic functions and calls on the SysV x86-64 baseline, including
LLVM `va_start`, `va_end`, `va_copy`, and `va_arg` lowering.

## Scope

- In: LLVM function `isVarArg`, default argument promotions at call boundary,
  compiler-provided stdarg hooks, and SysV `va_list` representation.
- Out: non-SysV `va_list` layouts.

## Deliverables

- `emit_va_start`, `emit_va_end`, `emit_va_copy`, `emit_va_arg`.
- Fixture: `sum(int n, ...)` differential against host cc.

## Acceptance

- A fixture summing `va_arg(ap, int)` returns the expected value.
- Variadic fixed parameters still use the ordinary ABI param classifier.

## References

- SysV x86-64 ABI 3.5.7
- LLVM LangRef: `va_arg`
