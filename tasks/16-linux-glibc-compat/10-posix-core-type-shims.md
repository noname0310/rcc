# 16-10: POSIX Core Type Shims

> ✓ done — 2026-05-06

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-09-pthread-header-shim  
**Milestone:** hosted-linux

## Goal

Cover the POSIX base types and declarations that repeatedly appear in hosted
project headers.

## Scope

- In: `sys/types.h`, `unistd.h`, `time.h`, `signal.h`, `errno.h`, and the type
  names from the audit.
- In: compile-only tests and one runtime smoke where appropriate.
- Out: kernel ABI recreation beyond the selected hosted surface.

## Acceptance

- [x] The audited type names have one canonical source in rcc resources or host
      headers.
- [x] `pid_t`, `size_t`, `ssize_t`, `off_t`, `time_t`, and signal handler forms
      parse and lower.
- [x] No shim changes object layout assumptions without a target-info entry.
- [x] GNU coreutils `true.c` advances past core POSIX type parsing.
