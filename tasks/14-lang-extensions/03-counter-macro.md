# 14-03: `__COUNTER__` predefined macro

**Phase:** 14-lang-extensions    **Depends on:** —    **Milestone:** M5

## Goal
Add `__COUNTER__` as a predefined macro that expands to a
monotonically increasing integer starting at 0, unique per
translation unit. Each expansion increments the counter.

## Scope
- In: extend the builtin macro table in `rcc_preprocess` with a
  `__COUNTER__` entry backed by a `Cell<u32>` counter.
- Out: cross-TU counter synchronisation (not applicable).

## Deliverables
- `__COUNTER__` registration in builtin macro table.
- Test: three consecutive uses expand to `0`, `1`, `2`.

## Acceptance
- `__COUNTER__` expands to `0` on first use and increments on
  each subsequent expansion within the same translation unit.
- Using `__COUNTER__` in different `#include`d files continues the
  same counter.

## References
- GCC/Clang extension; widely used for generating unique
  identifiers in macros.
