# 15-03: Compiler-provided `stdarg.h`

**Phase:** 15-builtin-rt    **Depends on:** 15-01, 15-05    **Milestone:** M5

## Goal
Ship a compiler-provided `lib/rcc/include/stdarg.h` implementing
`va_list`, `va_start`, `va_end`, `va_arg`, and `va_copy` via
`__builtin_va_*` builtins. The `va_list` type definition is
target-dependent.

## Scope
- In: `va_list` type: on SysV x86-64 it is a 1-element array of
  a struct with `gp_offset`, `fp_offset`, `overflow_arg_area`,
  `reg_save_area`; on Win64 and most 32-bit targets it is
  `char *`. Macros expand to `__builtin_va_start(ap, last)`, etc.
  `va_copy` expands to `__builtin_va_copy(dest, src)`.
- Out: implementing the `__builtin_va_*` intrinsics themselves
  (task 15-05 — this task only provides the header).

## Deliverables
- `lib/rcc/include/stdarg.h` with target-conditional `va_list`
  definition.
- Test: `#include <stdarg.h>` compiles, `va_list ap;` declares.

## Acceptance
- `#include <stdarg.h>` resolves to the compiler-provided header.
- `va_list` has the correct layout for the current target.
- `va_start(ap, last)` expands to `__builtin_va_start(ap, last)`.

## References
- C99 §7.15 — Variable arguments `<stdarg.h>`.
- SysV x86-64 ABI §3.5.7 (`va_list` layout).
