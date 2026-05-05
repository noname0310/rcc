# 16-22a: GNU extension Inline Header Functions

> ✓ done — 2026-05-06

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

- [x] A reduced `__extension__ static __inline` function definition fixture
      parses in hosted Linux mode.
- [x] Strict C99 mode preserves recovery and emits a targeted GNU-extension
      warning.
- [x] The coreutils `run-true-probe.sh` no longer reports an uncoded
      `expected ';' after declaration` failure from glibc `<bits/byteswap.h>`.

## Result

- Added parser support for GNU `__extension__` declaration prefixes.
- Added W0034 and documentation for strict C99 mode; hosted Linux mode
  suppresses the warning because glibc uses the marker specifically to avoid
  pedantic diagnostics in system headers.
- Added reduced glibc `<bits/byteswap.h>` fixtures for hosted and strict C99
  modes.
- Re-ran the GNU coreutils `src/true` probe: the glibc `<bits/byteswap.h>`
  syntax blocker is gone and the first compiler-owned failures are now the
  E0071/E0083 hosted declaration gaps tracked by task 16-23.
