> ✓ done — 2026-05-05

# 15-13: Hosted math declaration sweep

**Phase:** 15-builtin-rt    **Depends on:** 15-11    **Milestone:** real-world-03

## Goal
Expand `<math.h>` from the current small double-only subset to the C99 math
function-family declarations needed by real-world numeric libraries.

## Scope
- In: missing double declarations first, then `f`/`l` suffixed float and long
  double variants where the frontend can parse and type-check them.
- In: math classification/comparison macros only if they can be expressed in
  terms the current preprocessor/type checker supports.
- Out: implementing libm bodies and complex math.

## Acceptance
- [x] Add a hosted math fixture linked with `-lm`.
- [x] The fixture covers at least one function from each newly added family.
- [x] If a macro cannot be represented safely yet, document the blocker and add the
  compiler task instead of faking the macro.
