# 16-08: Pthread Driver Flag

> ✓ done — 2026-05-06

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-07-restrict-and-qualifier-aliases  
**Milestone:** hosted-linux

## Goal

Implement `-pthread` as a hosted compiler driver feature, matching the normal
compile-and-link contract used by Linux C compilers.

## Scope

- In: define `_REENTRANT` during preprocessing.
- In: pass the correct linker driver option or library flags during final link.
- In: tests that inspect driver planning and run a tiny pthread program on
  WSL/Linux when available.
- Out: implementing pthread functions inside rcc.

## Acceptance

- [x] `rcc -pthread -E` exposes `_REENTRANT`.
- [x] Link planning includes pthread support exactly once.
- [x] A pthread smoke program compiles, links, and runs on Linux.
- [x] Windows unsupported behavior is explicit and not a silent success.
