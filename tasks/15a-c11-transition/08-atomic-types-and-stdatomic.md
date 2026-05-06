# 15a-08: Atomic Types and `stdatomic.h`

**Phase:** 15a-c11-transition  
**Depends on:** 15a-07-generic-selection  
**Milestone:** c11-transition

## Goal

Add the C11 `_Atomic` type surface and a minimal `<stdatomic.h>` that can serve
real-world hosted projects while leaving full lock-free/runtime details
explicitly tracked.

## Scope

- In: parse `_Atomic(type-name)` as an atomic type specifier.
- In: parse `_Atomic` as a type qualifier where C11 permits it.
- In: represent atomic qualification/type wrapping in HIR.
- In: typeck prevents invalid atomic object types.
- In: codegen maps simple atomic loads/stores/fetch operations used by
  existing tests to LLVM atomics or well-defined builtin calls.
- In: `<stdatomic.h>` macros/types for `atomic_int`, `atomic_bool`,
  `memory_order`, `atomic_load`, `atomic_store`, `atomic_fetch_add`, and
  compare-exchange basics.
- Out: full C11 memory model proof, every optional lock-free macro, and
  non-hosted atomic runtime library bodies.

## Acceptance

- [ ] `_Atomic(int) x;` and `_Atomic int y;` parse in C11 mode.
- [ ] Existing QuickJS atomic smoke remains green.
- [ ] A C11-only atomic fixture compiles, links, and runs on Linux when LLVM is
      enabled.
- [ ] Unsupported atomic operations fail with targeted diagnostics rather than
      silently degrading to non-atomic operations.

## References

- N1570 6.7.2.4 atomic type specifier.
- N1570 7.17 `stdatomic.h`.
- Clang C11 atomic operations notes.
