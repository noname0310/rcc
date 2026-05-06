# 15a-13: Real Host C11 Library Headers

> ✓ done — 2026-05-06

**Phase:** 15a-c11-transition  
**Depends on:** 15a-12-c11-conformance-and-realworld-gates  
**Milestone:** c11-transition

## Goal

Keep C11 coverage while removing approximate libc/POSIX/Linux header shims from
`lib/rcc/include`.  rcc must continue to own C11 language/compiler surfaces, but
hosted C11 library headers must come from the real target sysroot.

## Scope

- In: remove checked-in C11 library declaration shims that duplicate host libc
  headers, including `assert.h`, `threads.h`, `uchar.h`, `time.h`, and
  `stdlib.h`.
- In: keep compiler-owned C11 headers such as `stdalign.h`, `stdnoreturn.h`,
  `stdatomic.h`, plus scalar/builtin headers (`stddef.h`, `stdarg.h`,
  `stdint.h`, `stdbool.h`, `iso646.h`, `limits.h`, `float.h`).
- In: Linux hosted tests must exercise real host C11 library headers under
  `--linux-gnu-hosted -std=c11`.
- In: non-hosted/unit tests may use explicit minimal declarations when the test
  is about rcc language lowering rather than libc header parsing.
- Out: restoring fake `lib/rcc/include` shims for glibc, POSIX, Linux kernel,
  or C library declarations.
- Out: implementing libc function bodies; host libc/libpthread/libm own runtime
  behavior.

## Acceptance

- [ ] `lib/rcc/include` contains only compiler-owned headers.
- [ ] `cargo test -p rcc_preprocess` confirms include ordering is project `-I`,
      rcc compiler-owned headers, then host system paths.
- [ ] Linux/WSL hosted header gate covers real host C11 library headers,
      including `assert.h`, `threads.h`, `uchar.h`, `time.h`, and `stdlib.h`,
      without using rcc shims for those names.
- [ ] Any real host-header failure becomes a minimized compiler regression in
      lexer, preprocessor, parser, HIR lowering, typeck, CFG, or driver tests.
- [ ] `docs/hosted-linux.md` and `docs/standard-header-surface.md` explain the
      C11 ownership split clearly.

## Notes

This task supersedes the shim-based parts of `15a-11` and the phase-16 shim
tasks.  It does not reduce the C11 target; it changes the oracle from
hand-written declarations to the real hosted sysroot.
