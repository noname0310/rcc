> ✓ done — 2026-05-05

# 15-12: Hosted core declaration sweep

**Phase:** 15-builtin-rt    **Depends on:** 15-11    **Milestone:** real-world-02

## Goal
Bring the already-present core hosted headers closer to C99 declaration
coverage in one reviewable sweep.

## Scope
- In:
  - `<stdio.h>`: file operations, buffering, formatted I/O including `v*`
    variants, character I/O, positioning, and error-state APIs.
  - `<stdlib.h>`: numeric conversion, random, environment, search/sort,
    integer arithmetic helpers, and multibyte conversion declarations.
  - `<string.h>`: remaining memory/string operations and strerror.
- Out: function bodies, POSIX-only APIs (`strdup`, `getline`, `fdopen`, etc.),
  and glibc internals.

## Acceptance
- [x] Add compile/link/run fixtures that exercise representative declarations from
  each touched header against host libc.
- [x] `cJSON` stage 1 still compiles and runs without relying on ad hoc header
  edits.
- [x] `docs/hosted-c99-header-audit.md` is updated with the post-sweep status.
