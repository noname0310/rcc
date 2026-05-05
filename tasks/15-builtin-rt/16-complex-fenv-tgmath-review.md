# 15-16: Complex, fenv, and tgmath support review

> ✓ done — 2026-05-05

**Phase:** 15-builtin-rt    **Depends on:** 15-14, 15-15    **Milestone:** real-world-03

## Goal
Decide and implement the sound path for the remaining hosted C99 headers that
need frontend semantics before declaration shims are useful.

## Scope
- In: `complex.h`, `fenv.h`, and `tgmath.h`.
- In: compiler/type-system support audit for `_Complex`, floating-point
  environment access pragmas, and type-generic macro dispatch.
- In: explicit subtasks for any parser, typeck, lowering, or codegen blockers.
- Out: copying host system headers or adding macros that only make includes
  parse while calls lower incorrectly.

## Acceptance
- [x] `complex.h`, `fenv.h`, and `tgmath.h` each have either a sound shim plus
  fixture, or an explicit blocked status tied to a compiler-support task.
- [x] `_Complex` and `tgmath.h` are not faked with declarations that hide missing
  type semantics.
- [x] Any accepted header has at least one compile/link fixture; runtime fixtures
  are required when behavior can be checked portably.

## Review result

- `fenv.h`: accepted as a Linux hosted ABI shim backed by host libm; covered by
  `hosted_fenv`.
- `complex.h`: blocked by task 15-17. rcc has `_Complex` scalar support, but
  lacks a sound way to expose C99's `I` / `_Complex_I` imaginary unit macro.
- `tgmath.h`: blocked by task 15-18. C99 type-generic dispatch across
  real/complex float families must not be faked with double-only declarations.
