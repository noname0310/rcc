# 15a-11: C11 Library Header Sweep

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

- [ ] A driver header gate includes every C11 resource header under
      `-std=c11`.
- [ ] Header declarations lower to HIR without requiring GNU flags.
- [ ] Every declaration-only runtime dependency is documented as host-owned.
- [ ] `docs/hosted-linux.md` and task docs list which C11 library features are
      implemented, declaration-only, or deferred.

## References

- N1570 library clauses 7.15, 7.17, 7.23, 7.26, and 7.28.
