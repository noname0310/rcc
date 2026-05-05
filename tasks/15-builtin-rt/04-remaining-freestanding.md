> ✓ done — 2026-05-05

# 15-04: Remaining freestanding headers

**Phase:** 15-builtin-rt    **Depends on:** 15-01    **Milestone:** M5

## Goal
Ship the remaining C99 freestanding headers under
`lib/rcc/include/`: `stdint.h`, `stdbool.h`, `limits.h`,
`float.h`, and `iso646.h`. Values are derived from `TargetInfo`.

## Scope
- In:
  - `stdint.h`: exact-width types (`int8_t` .. `int64_t`,
    `uint8_t` .. `uint64_t`), least/fast types, `intmax_t`,
    `uintmax_t`, `intptr_t`, `uintptr_t`, and all associated
    `_MIN` / `_MAX` macros.
  - `stdbool.h`: `bool`, `true`, `false`, `__bool_true_false_are_defined`.
  - `limits.h`: `CHAR_BIT`, `SCHAR_MIN/MAX`, `UCHAR_MAX`,
    `SHRT_MIN/MAX`, `USHRT_MAX`, `INT_MIN/MAX`, `UINT_MAX`,
    `LONG_MIN/MAX`, `ULONG_MAX`, `LLONG_MIN/MAX`, `ULLONG_MAX`,
    all derived from `TargetInfo` type sizes.
  - `float.h`: `FLT_RADIX`, `FLT_EPSILON`, `FLT_MIN`, `FLT_MAX`,
    `FLT_DIG`, `FLT_MANT_DIG`, and double/long-double variants.
  - `iso646.h`: alternative operator spellings (`and`, `or`, etc.).
- Out: `stdnoreturn.h`, `stdalign.h` (C11 — future).

## Deliverables
- Five header files under `lib/rcc/include/`.
- Tests: include each header, use at least one symbol from each.

## Acceptance
- `#include <stdint.h>` defines `int32_t` as a 32-bit signed
  integer on all targets.
- `#include <stdbool.h>` allows `bool b = true;`.
- `INT_MAX` from `limits.h` equals `2147483647` on ILP32/LP64.

## References
- C99 §7.18 (`stdint.h`), §7.16 (`stdbool.h`), §7.10 (`limits.h`),
  §7.7 (`float.h`), §7.9 (`iso646.h`).
