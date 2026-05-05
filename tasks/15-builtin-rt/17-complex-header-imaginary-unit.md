# 15-17: Complex header imaginary unit support

**Phase:** 15-builtin-rt    **Depends on:** 15-16    **Milestone:** real-world-03

## Goal
Add `complex.h` only after rcc can represent C99's imaginary unit macros
soundly.

## Scope
- In: decide between a compiler builtin such as `__builtin_complex(real, imag)`
  and full `_Imaginary` support.
- In: `complex.h` macros `complex`, `_Complex_I`, `I`, and optional
  `imaginary` / `_Imaginary_I` only when backed by compiler semantics.
- In: C99 complex function declarations (`cabs`, `carg`, `cexp`, `creal`,
  `cimag`, `conj`, `cproj`, and float/long-double variants) once construction
  and calls are codegen-safe.
- In: compile/link/run fixture that constructs a non-zero imaginary component
  and verifies `creal` / `cimag`.
- Out: defining `I` as a pointer, integer, double-only placeholder, or any
  expression that loses the imaginary component.

## Acceptance
- `#include <complex.h>` exposes a standard-compatible imaginary unit.
- `double complex z = 2.0 + 3.0 * I;` compiles, links, runs, and verifies real
  and imaginary parts.
- Complex function declarations are ABI-compatible with host libm on the
  current Linux target.
- If `_Imaginary` remains unsupported, the header does not expose
  `_Imaginary_I`.

