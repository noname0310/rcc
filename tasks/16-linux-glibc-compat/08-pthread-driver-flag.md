# 16-08: Pthread Driver Flag

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

- [ ] `rcc -pthread -E` exposes `_REENTRANT`.
- [ ] Link planning includes pthread support exactly once.
- [ ] A pthread smoke program compiles, links, and runs on Linux.
- [ ] Windows unsupported behavior is explicit and not a silent success.
