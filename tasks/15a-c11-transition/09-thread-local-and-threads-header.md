# 15a-09: `_Thread_local` and `threads.h`

> ✓ done — 2026-05-06

**Phase:** 15a-c11-transition  
**Depends on:** 15a-08-atomic-types-and-stdatomic  
**Milestone:** c11-transition

## Goal

Support the C11 thread-local storage specifier and provide declaration-level
coverage for `<threads.h>` without pretending that `rcc` owns the thread
runtime.

## Scope

- In: parse `_Thread_local` as a storage-class specifier.
- In: HIR/linkage representation for thread-local objects.
- In: LLVM codegen emits target TLS storage for simple globals.
- In: `<threads.h>` declarations for `thrd_*`, `mtx_*`, `cnd_*`, `tss_*`, and
  `call_once` sufficient for hosted Linux compile/link smoke tests.
- In: map runtime bodies to host libc/pthread where available.
- Out: implementing a standalone C11 threads runtime.
- Out: making C11 threads mandatory on targets whose host runtime does not
  provide it.

## Acceptance

- [x] `_Thread_local int x;` emits TLS LLVM IR on x86_64 Linux.
- [x] `thread_local` from a C11 resource header maps to `_Thread_local`.
- [x] `<threads.h>` parses and lowers under `-std=c11 --linux-gnu-hosted`.
- [x] A Linux smoke test using `thrd_create` either links/runs or records a
      precise host-runtime blocker.

## References

- N1570 6.7.1 storage-class specifiers.
- N1570 7.26 `threads.h`.
