> ✓ done — 2026-05-05

# 15-05: `__builtin_va_*` functions

**Phase:** 15-builtin-rt    **Depends on:** 15-01    **Milestone:** M5

## Goal
Recognize `__builtin_va_start`, `__builtin_va_end`,
`__builtin_va_arg`, and `__builtin_va_copy` in name resolution and
lower them to the corresponding LLVM `va_start`, `va_end`,
`va_arg`, and `va_copy` intrinsics.

## Scope
- In: builtin name recognition in `rcc_hir_lower` or a dedicated
  builtins table. Type-checking: `__builtin_va_start(ap, last)`
  requires `ap` to be a `va_list`, `last` must be the last named
  parameter. Codegen: emit LLVM `@llvm.va_start`, `@llvm.va_end`,
  `@llvm.va_copy` intrinsic calls and `va_arg` instruction.
- Out: full variadic calling convention (already in 09-13);
  this task provides the intrinsic lowering.

## Deliverables
- Builtin function table with `__builtin_va_*` entries.
- HIR lowering for builtin calls.
- LLVM codegen for `va_start`, `va_end`, `va_arg`, `va_copy`.
- Test: variadic function using `va_start`/`va_arg`/`va_end`.

## Acceptance
- A variadic `int sum(int n, ...)` function using `va_start` /
  `va_arg` / `va_end` compiles to valid LLVM IR.
- `va_copy` produces correct LLVM IR.

## References
- LLVM Language Reference: `va_arg` instruction, `va_start`/
  `va_end`/`va_copy` intrinsics.
- C99 §7.15.
