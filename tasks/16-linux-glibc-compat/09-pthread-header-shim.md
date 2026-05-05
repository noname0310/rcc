# 16-09: Pthread Header Shim

> ✓ done — 2026-05-06

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-08-pthread-driver-flag  
**Milestone:** hosted-linux

## Goal

Make pthread declarations parse and type-check well enough for hosted Linux
projects while still linking to host libpthread/glibc.

## Scope

- In: `pthread_t`, `pthread_attr_t`, mutex/cond types needed by probes, and
  declarations for create/join/mutex basics.
- In: source-level tests and runtime smoke through host pthread.
- Out: pthread implementation or scheduler behavior.

## Acceptance

- [x] A minimal program using `pthread_create` and `pthread_join` compiles.
- [x] The same program links and runs with `rcc -pthread` on Linux.
- [x] The shim does not conflict with host `<pthread.h>` when the host header is
      parseable.
- [x] The task documents which types are opaque placeholders.
