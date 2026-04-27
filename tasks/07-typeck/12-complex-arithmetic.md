> ✓ done — 2026-04-27

# 07-12: `_Complex` arithmetic

**Phase:** 07-typeck    **Depends on:** 07-02    **Milestone:** M7

## Goal
Implement type-checking and implicit conversions for `_Complex`
types: `_Complex float`, `_Complex double`, `_Complex long double`.
Cover arithmetic operations and conversions between complex and
real types per C99 §6.3.1.6.

## Scope
- In: usual arithmetic conversions extended for complex types
  (C99 §6.3.1.8 with complex). Binary `+`, `-`, `*`, `/` on
  complex operands. Conversions: real → complex (imaginary part
  becomes 0), complex → real (discard imaginary part with warning
  if non-zero), complex → complex (convert both parts).
  `__real__` and `__imag__` unary operators (GCC extension).
- Out: `_Imaginary` type (C99 Annex G — rarely implemented).
  Complex I/O, `<complex.h>` (libc).

## Deliverables
- Extend `usual_arithmetic_conversion` for complex operands.
- Implicit conversion insertion for complex ↔ real.
- Type-check `+`, `-`, `*`, `/` with complex operands.
- Tests: truth table for complex conversions.

## Acceptance
- `_Complex double a = 1.0 + 2.0i; _Complex double b = a * a;`
  type-checks.
- `double r = (_Complex double)3.0;` inserts real → complex
  conversion.
- Assigning complex to real emits a warning about discarded
  imaginary part.

## References
- C99 §6.2.5¶11 (complex types), §6.3.1.6 (complex conversions),
  §6.3.1.8 (usual arithmetic with complex), §6.5 (operators).
