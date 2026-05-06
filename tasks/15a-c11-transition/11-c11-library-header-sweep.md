# 15a-11: C11 Library Header Sweep

> ✓ done — 2026-05-06

**Phase:** 15a-c11-transition  
**Depends on:** 15a-10-unicode-character-and-string-literals  
**Milestone:** c11-transition

## Goal

Bring compiler-owned resource headers up to a coherent C11 declaration surface
without copying large libc headers.

## Scope

- In: add or update `stdalign.h`, `stdnoreturn.h`, `stdatomic.h`,
  `threads.h`, and `uchar.h`.
- In: audit existing C99 headers for C11 macro deltas, especially `<float.h>`,
  `<stdlib.h>`, `<assert.h>`, and `<time.h>`.
- In: document optional/deferred pieces: Annex K bounds-checking interfaces,
  analyzability annex, and fully conforming C11 thread runtime.
- In: Linux hosted mode can still rely on host runtime bodies.
- Out: wholesale import of glibc/musl headers.

## Acceptance

- [x] A driver header gate includes every C11 resource header under
      `-std=c11`.
- [x] Header declarations lower to HIR without requiring GNU flags.
- [x] Every declaration-only runtime dependency is documented as host-owned.
- [x] `docs/hosted-linux.md` and task docs list which C11 library features are
      implemented, declaration-only, or deferred.

## Completed Surface

This task was originally completed with small rcc-owned declaration shims for
some C11 library headers. That policy has been superseded by
`15a-13-real-host-c11-library-headers.md`: C11 coverage remains required, but
hosted C11 library headers now come from the real target sysroot instead of
approximate files under `lib/rcc/include`.

- Implemented small C11 resource-header deltas:
  - `stdnoreturn.h`: `noreturn`, `__noreturn_is_defined`.
  - `float.h`: decimal-digit/subnormal macros for current target baselines.
  - `stdatomic.h`: full scalar typedef surface, `atomic_flag`, init macros,
    lock-free probes, and declaration-only `atomic_flag_*` functions.
- Added a hosted Linux driver fixture that includes C11 library headers under
  `-std=c11 -pthread`; after task 15a-13 those names are expected to resolve
  from real host headers, not rcc shims.
- Documented host-owned runtime behavior and deferred Annex K/analyzability/
  full thread-runtime work in `docs/hosted-linux.md`.

## References

- N1570 library clauses 7.15, 7.17, 7.23, 7.26, and 7.28.
