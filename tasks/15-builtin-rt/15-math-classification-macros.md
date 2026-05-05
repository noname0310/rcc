# 15-15: Math classification and comparison macros

**Phase:** 15-builtin-rt    **Depends on:** 15-13    **Milestone:** real-world-03

## Goal
Represent C99 `<math.h>` classification and comparison macros without faking
runtime behavior in the declaration shim.

## Scope
- In: `fpclassify`, `isfinite`, `isinf`, `isnan`, `isnormal`, `signbit`,
  `isgreater`, `isgreaterequal`, `isless`, `islessequal`, `islessgreater`,
  and `isunordered`.
- In: required constants such as `FP_NAN`, `FP_INFINITE`, `FP_ZERO`,
  `FP_SUBNORMAL`, `FP_NORMAL`, `HUGE_VAL`, `HUGE_VALF`, `HUGE_VALL`,
  `INFINITY`, and `NAN` only when represented with sound frontend semantics.
- Out: compiler-internal floating-point classification implementation unless
  the macro expansion needs it.

## Acceptance
- Add compile/link/run fixtures covering classification and comparison macros.
- If builtin lowering is required, add it before exposing the macros.
- Do not define these macros as arbitrary constants merely to unblock a project.

