# 15-08: Builtin runtime integration tests

**Phase:** 15-builtin-rt    **Depends on:** 15-01 through 15-07    **Milestone:** M6

## Goal
End-to-end integration tests that exercise the compiler-provided
headers and builtin functions together. Verify that real-world
patterns using freestanding headers and builtins compile and
produce correct results.

These tests may link against the host libc/CRT for hosted functions,
but they must not assume rcc implements libc function bodies itself.

## Scope
- In: test files that `#include <stdint.h>`, `#include <stdarg.h>`,
  `#include <stddef.h>`, `#include <stdbool.h>`, `#include <limits.h>`;
  use `va_start` / `va_arg` in a variadic function; use
  `offsetof(type, member)`; use `size_t` and `NULL`; use `INT_MAX`
  in a static assertion. Compile, link, and run each test.
- Out: tests requiring rcc-owned implementations of hosted libc functions.
  Hosted calls may be smoke-tested only when they link against the
  platform libc/CRT through the normal driver path.

## Deliverables
- `tests/builtin-rt/` directory with C test files.
- Integration test runner that compiles and runs each file.
- At least 5 test files covering headers, va_args, offsetof,
  type sizes, and builtin functions.

## Acceptance
- All test files compile without errors.
- Variadic function test produces correct output at runtime.
- `offsetof` test produces correct byte offsets.
- Static assertions on type sizes and limits pass.

## References
- C99 §4 — Conformance (freestanding requirements).
