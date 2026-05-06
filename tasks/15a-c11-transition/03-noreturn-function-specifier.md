> ✓ done — 2026-05-06

# 15a-03: `_Noreturn` Function Specifier

**Phase:** 15a-c11-transition  
**Depends on:** 15a-02-c11-keyword-tokenization  
**Milestone:** c11-transition

## Goal

Implement `_Noreturn` as a real C11 function specifier and remove the need for
project wrappers to pass `-D_Noreturn=`.

## Scope

- In: extend AST `FuncSpecs` with `noreturn`.
- In: parse `_Noreturn` in declaration-specifier lists alongside `inline`.
- In: lower `_Noreturn` into HIR `CommonAttrs::noreturn`, merging with
  `__attribute__((noreturn))`.
- In: LLVM codegen keeps emitting the `noreturn` function attribute.
- In: C11 regression tests for declarations, definitions, redeclarations, and
  function pointer compatibility policy.
- In: update hosted Toybox wrapper to stop defining `_Noreturn`.
- Out: full control-flow unreachable analysis after calls to noreturn
  functions unless existing CFG support already makes it cheap.

## Acceptance

- [ ] `_Noreturn void f(void);` parses in `-std=c11`.
- [ ] `_Noreturn void f(void) { for (;;) {} }` lowers to HIR with
      `CommonAttrs::noreturn`.
- [ ] LLVM IR for a `_Noreturn` function contains `noreturn`.
- [ ] `-std=c99` behavior is explicit and tested.
- [ ] Toybox no longer uses a preprocessor `_Noreturn` workaround.

## References

- N1570 6.7.4 function specifiers.
- N1570 7.23 `stdnoreturn.h`.
