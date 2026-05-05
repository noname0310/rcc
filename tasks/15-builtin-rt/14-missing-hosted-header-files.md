# 15-14: Missing hosted header files

**Phase:** 15-builtin-rt    **Depends on:** 15-11    **Milestone:** real-world-03

## Goal
Add minimal declaration-shim header files for C99 hosted headers that are
currently absent from `lib/rcc/include/`.

## Scope
- In: `assert.h`, `errno.h`, `inttypes.h`, `locale.h`, `setjmp.h`, `signal.h`,
  `time.h`, and `wctype.h` as minimal ABI-facing shims.
- Evaluate separately: `complex.h`, `fenv.h`, and `tgmath.h`, because they may
  require compiler/type-system support before declarations are useful.
- Out: libc implementations, POSIX extensions, GNU extensions, and copying
  system headers.

## Acceptance
- Each new header has a small compile-only or compile/link fixture.
- Headers contain declarations/types/macros only; no function bodies.
- Any compiler support blocker becomes an explicit task before the header is
  marked complete.

