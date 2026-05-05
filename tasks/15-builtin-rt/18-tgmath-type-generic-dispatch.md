# 15-18: tgmath type-generic dispatch

> ✓ done — 2026-05-05

**Phase:** 15-builtin-rt    **Depends on:** 15-17    **Milestone:** real-world-03

## Goal
Implement `tgmath.h` with sound C99 type-generic dispatch instead of
double-only macro aliases.

## Scope
- In: dispatch for real float/double/long-double math families.
- In: dispatch for complex float/double/long-double families after
  `complex.h` is sound.
- In: a frontend or preprocessor-level mechanism that chooses the correct
  callee from expression type, not from textual spelling alone.
- Out: C11 `_Generic`, unless the project explicitly adds it as an extension.
- Out: macros that evaluate arguments more times than the corresponding
  function-family semantics allow.

## Acceptance
- [x] `sqrt(1.0F)`, `sqrt(1.0)`, and `sqrt(1.0L)` through `tgmath.h` call the
  float, double, and long-double variants respectively.
- [x] Complex arguments dispatch to complex libm functions.
- [x] A fixture demonstrates mixed real/complex dispatch without changing original
  source code.
- [x] `tgmath.h` stays absent or explicitly blocked until these conditions are met.
