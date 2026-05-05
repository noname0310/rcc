# 16-22a: GNU extension Inline Header Functions

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-22-gnulib-funcdecl-macro-surface  
**Milestone:** hosted-linux

## Goal

Parse GNU `__extension__` prefixes used by glibc inline header functions, such
as `__extension__ static __inline __uint64_t __bswap_64(...) { ... }`, without
falling out of declaration parsing.

## Scope

- In: GNU `__extension__` as a declaration prefix for hosted Linux headers.
- In: reduced fixtures from glibc `<bits/byteswap.h>`.
- In: strict-mode diagnostics if the token appears outside hosted/GNU mode.
- Out: broad GNU expression-statement semantics unless another real-world
  fixture requires it.

## Acceptance

- [ ] A reduced `__extension__ static __inline` function definition fixture
      parses in hosted Linux mode.
- [ ] Strict C99 mode preserves recovery and emits a targeted GNU-extension
      warning.
- [ ] The coreutils `run-true-probe.sh` no longer reports an uncoded
      `expected ';' after declaration` failure from glibc `<bits/byteswap.h>`.
