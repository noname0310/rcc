# 16-11: Fcntl Dirent Stat Shims

> ✓ done — 2026-05-06

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-10-posix-core-type-shims  
**Milestone:** hosted-linux

## Goal

Support file-system oriented POSIX declarations that dominate GNU userland
projects.

## Scope

- In: `fcntl.h`, `dirent.h`, `sys/stat.h`, selected `sys/time.h` and
  `sys/wait.h` declarations from the audit.
- In: object layout decisions for `struct stat` only if rcc must model them for
  compile-time layout; otherwise prefer host headers.
- Out: full kernel header coverage.

## Acceptance

- [x] Header smoke tests parse and type-check the selected file-system headers.
- [x] The chosen policy for opaque vs layout-known structs is documented.
- [x] At least one coreutils file-system utility source reaches the next
      non-header blocker.
- [x] No project source is edited.
