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

- Implemented small C11 resource-header deltas:
  - `stdnoreturn.h`: `noreturn`, `__noreturn_is_defined`.
  - `assert.h`: `static_assert` macro.
  - `float.h`: decimal-digit/subnormal macros for current target baselines.
  - `stdlib.h`: `aligned_alloc`, `quick_exit`, `at_quick_exit`.
  - `time.h`: `TIME_UTC`, `timespec_get`.
  - `stdatomic.h`: full scalar typedef surface, `atomic_flag`, init macros,
    lock-free probes, and declaration-only `atomic_flag_*` functions.
- Added a hosted Linux driver fixture that includes `assert.h`, `float.h`,
  `stdalign.h`, `stdatomic.h`, `stdnoreturn.h`, `stdlib.h`, `threads.h`,
  `time.h`, and `uchar.h` together under `-std=c11 -pthread` without GNU syntax
  flags.
- Documented host-owned runtime behavior and deferred Annex K/analyzability/
  full thread-runtime work in `docs/hosted-linux.md`.

## References

- N1570 library clauses 7.15, 7.17, 7.23, 7.26, and 7.28.
