# 08-28: string literal index place

> ✓ done — 2026-05-05

**Phase:** 08-cfg    **Depends on:** 08-04, 08-25    **Milestone:** M6+

## Goal

Lower string literals as addressable global places when they appear in lvalue
projection chains.

## Trigger

zlib `infback.c` reaches CFG with a condition containing a string literal
subscript. Typed HIR represents that shape as:

```text
Index {
  base: Convert(ArrayToPtr, StringRef(...)),
  index: ...
}
```

`StringRef` is an lvalue array backed by a synthetic global definition. The old
CFG path treated only `DefRef` as a global place and panicked when the array
subscript base unwrapped to `StringRef`.

## Scope

- In:
  - Treat `StringRef` as a global `Place` in `lower_as_place`.
  - Add a direct unit test for `"ab"[1]` style projection.
- Out:
  - String literal interning.
  - LLVM global materialization of string literal payloads.
  - Type checking of string literal decay.

## Acceptance

- [x] `StringRef` can appear in lvalue position without CFG panic.
- [x] `Index(ArrayToPtr(StringRef), 1)` lowers to `Global(string), Index(1)`.
- [x] Existing `DefRef` global-object lvalue behavior remains unchanged.
- [x] zlib `infback.c --emit=mir` progresses past the previous `lower_as_place` panic.

## References

- C99 §6.4.5 string literals
- C99 §6.3.2.1 array-to-pointer conversion
- `real_world/projects/03-zlib/plan.md`
