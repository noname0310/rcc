# 15-builtin-rt

**Goal of the phase.** Provide the compiler-owned support surface that
real C programs need: a target abstraction layer, freestanding standard
headers (`stddef.h`, `stdarg.h`, `stdint.h`, etc.), compiler builtin
lowering, and system header search path discovery.

## Non-goals

- Do not implement hosted libc/glibc/MSVCRT function bodies such as
  `printf`, `malloc`, `fopen`, or `memcpy`.
- Do not vendor or reimplement glibc headers wholesale.
- Hosted library symbols are provided by the target platform's libc/CRT
  at link time. rcc only needs declarations, builtin hooks, sysroot
  discovery, and linker-driver wiring.

## Current seed

Task 10-18 introduced a minimal `lib/rcc/include/` seed because
ordinary `#include` dispatch became live and parser/conformance smoke
tests must not hide standard-header failures by bulk-xfail. The seed is
parse-oriented: phase-15 tasks still own target-correct definitions,
builtin semantics, hosted system-header search, and codegen validation.
It is not a libc implementation.

## Tasks

| # | File | Summary |
|---|------|---------|
| 01 | [`01-target-info.md`](01-target-info.md) | `TargetInfo` struct and triple parsing. |
| 02 | [`02-stddef-header.md`](02-stddef-header.md) | Ship `stddef.h` (size_t, NULL, offsetof). |
| 03 | [`03-stdarg-header.md`](03-stdarg-header.md) | Ship `stdarg.h` (va_list, va_start, etc.). |
| 04 | [`04-remaining-freestanding.md`](04-remaining-freestanding.md) | Ship stdint.h, stdbool.h, limits.h, float.h, iso646.h. |
| 05 | [`05-builtin-va-functions.md`](05-builtin-va-functions.md) | `__builtin_va_*` → LLVM intrinsics. |
| 06 | [`06-builtin-common.md`](06-builtin-common.md) | offsetof, expect, unreachable, bswap, etc. |
| 07 | [`07-system-header-search.md`](07-system-header-search.md) | System include path discovery + `--sysroot`. |
| 08 | [`08-unit-tests.md`](08-unit-tests.md) | Integration tests for headers + builtins. |

## Exit criteria

- `#include <stdint.h>` resolves to the compiler-provided header
  and `int32_t` is usable.
- A variadic function using `va_start`/`va_arg` compiles and runs.
- `__builtin_offsetof(struct S, field)` evaluates at compile time.
- On Linux, `#include <stdio.h>` resolves via system header search.
- A `printf` hello-world links against the host libc/CRT; rcc does not
  provide the `printf` implementation.
