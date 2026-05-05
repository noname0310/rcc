# 16-23: coreutils POSIX Declaration Sweep

> ✓ done — 2026-05-06

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-22a-gnu-extension-inline-header-functions  
**Milestone:** hosted-linux

## Goal

Close the concrete hosted declaration and macro gaps surfaced by the GNU
coreutils `src/true.c` probe.

## Scope

- In: small rcc resource-header additions for declarations/macros that are
  host-runtime-owned: `wcwidth`, `fchownat`, `fchmodat`,
  `AT_SYMLINK_NOFOLLOW`, `S_TYPEISSHM`, `S_TYPEISTMO`, `fputs_unlocked`,
  `fwrite_unlocked`, `fflush_unlocked`, `clearerr_unlocked`, `fpurge`,
  `vasprintf`, `EOPNOTSUPP`, and `ENOTSUP`.
- In: reduced header parse/typecheck tests justifying each shim.
- Out: implementing libc or gnulib function bodies.

## Acceptance

- [x] Every added shim cites the coreutils log source in docs.
- [x] Hosted Linux header tests cover the new declarations/macros.
- [x] The coreutils `run-true-probe.sh` no longer reports E0071/E0083 for the
      names listed above.

## Result

- Added host-owned declaration/macro shims to `stdio.h`, `unistd.h`,
  `fcntl.h`, `sys/stat.h`, `errno.h`, and `wchar.h`.
- Added a hosted Linux header gate fixture covering the concrete coreutils
  declaration sweep surface.
- Re-ran the GNU coreutils `src/true` probe: `true.hir` is produced and
  E0071/E0083 no longer appears for the listed names.
