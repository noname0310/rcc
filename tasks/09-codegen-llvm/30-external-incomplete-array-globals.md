# 09-30: external incomplete array globals

> ✓ done — 2026-05-05

**Phase:** 09-codegen-llvm    **Depends on:** 09-06, 09-11    **Milestone:** M6+

## Goal

Allow external file-scope array declarations with unknown bound to reach LLVM
IR without requiring an object size in the current translation unit.

## Trigger

zlib headers declare objects such as:

```c
extern const char deflate_copyright[];
```

The declaration is valid C: the object is defined in another translation unit,
so the current module only needs an external symbol declaration. The previous
LLVM lowering path tried to lower the incomplete array as an object type and
failed before zlib source files could be code-generated.

## Scope

- In:
  - Detect external globals with incomplete non-VLA array type and no initializer.
  - Emit a zero-length LLVM array placeholder (`[0 x T]`) for the declaration.
  - Preserve the existing error for incomplete arrays that need storage in this
    translation unit.
- Out:
  - Completing tentative incomplete array definitions.
  - Flexible array member lowering.
  - Runtime use of incomplete arrays without array-to-pointer decay.

## Acceptance

- [x] `extern char x[];` lowers to an external LLVM global declaration.
- [x] The declaration has no initializer.
- [x] Non-external incomplete arrays still use the existing object-size error path.
- [x] zlib `adler32.c` progresses past the previous incomplete-array declaration failure.

## References

- C99 §6.2.5 incomplete types
- C99 §6.9.2 external definitions
- `real_world/projects/03-zlib/plan.md`
