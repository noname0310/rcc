# 15-19: POSIX errno constants

> ✓ done — 2026-05-05

**Phase:** 15-builtin-rt    **Depends on:** 15-14    **Milestone:** M6+

## Goal

Expose the common Linux/POSIX `errno.h` constants from the compiler-owned
hosted header shim.

## Trigger

LibTomMath `s_mp_rand_platform.c` includes `<errno.h>` and tests `errno ==
EINTR` around `open`, `read`, and `getrandom`. `rcc` resolves `<errno.h>` to
its builtin resource header before host system headers, but that shim only
defined the C99-required `EDOM`, `EILSEQ`, and `ERANGE` constants. As a result,
`EINTR` stayed unexpanded and HIR lowering reported an undeclared identifier.

## Scope

- In:
  - Add the common Linux/POSIX numeric errno constants needed by hosted library
    code, including `EINTR`, `EINVAL`, and `ENOMEM`.
  - Keep the existing `errno` lvalue declaration behavior.
  - Extend the hosted header runtime fixture to assert representative constants.
- Out:
  - Full libc header implementation.
  - Target-specific Windows errno value modeling.
  - System-header passthrough instead of builtin resource headers.

## Acceptance

- [x] `#include <errno.h>` expands `EINTR`.
- [x] The builtin hosted header fixture compiles, links, and runs.
- [x] LibTomMath `s_mp_rand_platform.c --emit=llvm-ir` compiles.
- [x] The LibTomMath TU IR smoke progresses past the previous `EINTR` blocker.

## References

- C99 §7.5 `errno.h`
- Linux generic errno ABI
- `real_world/projects/04-libtommath/plan.md`
