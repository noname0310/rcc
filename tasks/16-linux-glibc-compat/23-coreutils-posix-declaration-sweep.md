# 16-23: coreutils POSIX Declaration Sweep

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

- [ ] Every added shim cites the coreutils log source in docs.
- [ ] Hosted Linux header tests cover the new declarations/macros.
- [ ] The coreutils `run-true-probe.sh` no longer reports E0071/E0083 for the
      names listed above.
