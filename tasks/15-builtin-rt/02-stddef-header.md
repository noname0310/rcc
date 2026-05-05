> ✓ done — 2026-05-05

# 15-02: Compiler-provided `stddef.h`

**Phase:** 15-builtin-rt    **Depends on:** 15-01    **Milestone:** M5

## Goal
Ship a compiler-provided `lib/rcc/include/stddef.h` that defines
`size_t`, `ptrdiff_t`, `NULL`, `offsetof(type, member)`,
`wchar_t`, and `max_align_t`. Automatically prepend
`lib/rcc/include/` to the include search path so it is found
before system headers.

## Scope
- In: `size_t` as `unsigned long` (LP64) or `unsigned long long`
  (LLP64) depending on target. `ptrdiff_t` similarly. `NULL` as
  `((void *)0)`. `offsetof` via `__builtin_offsetof(type, member)`.
  `wchar_t` as `int` (Linux) or `unsigned short` (Windows).
  `max_align_t` matching target's maximum fundamental alignment.
  Add compiler include path to search order.
- Out: actually implementing `__builtin_offsetof` (task 15-06).

## Deliverables
- `lib/rcc/include/stddef.h` file.
- Include search path automatically includes compiler headers dir.
- Test: `#include <stddef.h>` compiles, `size_t x = sizeof(int);`
  type-checks.

## Acceptance
- `#include <stddef.h>` resolves to the compiler-provided header.
- `sizeof(size_t) == sizeof(void *)` on all supported targets.
- `offsetof(struct S, field)` expands without error.

## References
- C99 §7.17 — Common definitions `<stddef.h>`.
