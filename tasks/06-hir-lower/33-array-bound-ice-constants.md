# 06-33: array-bound integer constant expressions

> ✓ done — 2026-05-06

**Phase:** 06-hir-lower    **Depends on:** 06-19    **Milestone:** real-world/lua

## Goal

Classify record and file-scope arrays whose bounds are C99 integer constant
expressions as fixed arrays, not VLAs.

## Trigger

Lua declares fields such as `tmname[TM_N]`, `padding[offsetof(...)]`, and
scratch arrays whose bounds include casts and `sizeof`. HIR lowering previously
accepted only a narrow subset of constant expressions, so valid fixed-size
arrays were tagged as runtime VLAs. LLVM codegen then failed when it needed a
compile-time record layout.

## Scope

- In:
  - Enumerator identifiers in array bounds.
  - Cast wrappers around constant array bounds.
  - `__builtin_offsetof(type, field)` in array bounds.
  - Character literals in array bounds.
- Out:
  - General-purpose full expression constant evaluation before typeck.
  - Runtime VLA allocation semantics.

## Acceptance

- [x] `enum { N = 6 }; struct S { int a[N]; };` lowers `a` as `Array[6]`.
- [x] `char padding[__builtin_offsetof(struct Base, value)]` lowers as a fixed array.
- [x] `char scratch[(int)(16 * sizeof(void*))]` lowers as a fixed array.
- [x] Lua `lfunc.c`, `lmem.c`, `lobject.c`, `lauxlib.c`, `liolib.c`,
  `loadlib.c`, `ltablib.c`, `lutf8lib.c`, `ltable.c`, and `lstrlib.c`
  progress past the previous VLA layout failures.

## References

- C99 §6.6 constant expressions
- C99 §6.7.5.2 array declarators
- `real_world/projects/05-lua/plan.md`
