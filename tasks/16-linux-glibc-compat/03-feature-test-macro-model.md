# 16-03: Feature-Test Macro Model

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-02-compat-mode-and-policy  
**Milestone:** hosted-linux

## Goal

Model the feature-test macros expected by glibc and GNU userland builds without
hard-coding accidental project-specific behavior.

## Scope

- In: `_GNU_SOURCE`, `_DEFAULT_SOURCE`, `_POSIX_C_SOURCE`, `_XOPEN_SOURCE`,
  `_REENTRANT`, and the macro set implied by `-pthread`.
- In: compile-only tests that include common glibc headers under each mode.
- Out: libc body implementation.

## Acceptance

- [ ] `Session` or driver options preserve feature-test macros in the same path
      as normal `-D` definitions.
- [ ] `-pthread` implies `_REENTRANT` during preprocessing.
- [ ] Tests cover macro visibility differences for at least `<unistd.h>`,
      `<pthread.h>`, and `<features.h>` on WSL/Linux.
- [ ] The behavior is documented in `docs/hosted-linux.md`.
