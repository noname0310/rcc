# 15-20: stdlib exit status macros

> ✓ done — 2026-05-06

**Phase:** 15-builtin-rt    **Depends on:** 15-12    **Milestone:** real-world/lua

## Goal

Expose the C99-required status and utility macros from the compiler-owned
`stdlib.h` shim.

## Trigger

Lua's `loslib.c` and `lua.c` use `EXIT_SUCCESS` and `EXIT_FAILURE`. `rcc`
resolves `<stdlib.h>` to its builtin hosted header before host system headers,
but that shim only declared functions and typedefs. The macros stayed
unexpanded and HIR lowering reported undeclared identifiers.

## Scope

- In:
  - `EXIT_SUCCESS`
  - `EXIT_FAILURE`
  - `RAND_MAX`
  - `MB_CUR_MAX`
  - Hosted header fixture coverage for those macros.
- Out:
  - Implementing libc function bodies.
  - Locale-sensitive runtime value modeling for `MB_CUR_MAX`.

## Acceptance

- [x] `#include <stdlib.h>` expands `EXIT_SUCCESS` and `EXIT_FAILURE`.
- [x] The hosted builtin-header fixture compiles, links, and runs.
- [x] Lua `loslib.c` and `lua.c` progress past undeclared exit-status macros.

## References

- C99 §7.20 `stdlib.h`
- `real_world/projects/05-lua/plan.md`
