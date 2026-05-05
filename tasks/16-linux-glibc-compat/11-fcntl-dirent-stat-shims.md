# 16-11: Fcntl Dirent Stat Shims

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

- [ ] Header smoke tests parse and type-check the selected file-system headers.
- [ ] The chosen policy for opaque vs layout-known structs is documented.
- [ ] At least one coreutils file-system utility source reaches the next
      non-header blocker.
- [ ] No project source is edited.
