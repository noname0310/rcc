> ✓ done — 2026-05-05

# 14-06: Target-dependent predefined macros

**Phase:** 14-lang-extensions    **Depends on:** 15-01    **Milestone:** M6

## Goal
Inject target-dependent predefined macros into the preprocessor based
on `TargetInfo`. This enables portable code that conditionally compiles
for different architectures and operating systems.

## Scope
- In: architecture macros (`__x86_64__`, `__aarch64__`, `__i386__`),
  OS macros (`_WIN32`, `__linux__`, `__APPLE__`, `__unix__`),
  data-model macros (`__LP64__`, `__ILP32__`),
  `__SIZEOF_INT__` / `__SIZEOF_LONG__` / `__SIZEOF_POINTER__`,
  `__BYTE_ORDER__` / `__ORDER_LITTLE_ENDIAN__` / `__ORDER_BIG_ENDIAN__`,
  GCC compatibility (`__GNUC__`, `__GNUC_MINOR__`, `__GNUC_PATCHLEVEL__`),
  `__STDC_HOSTED__` (1 for hosted, 0 for freestanding).
- Out: feature-test macros (`_POSIX_SOURCE`, etc.) — libc's job.

## Deliverables
- `target_predefines(info: &TargetInfo) -> Vec<(Symbol, TokenStream)>`
  function.
- Registration in preprocessor init, after CLI defines.
- Test: cross-check `__SIZEOF_POINTER__` equals pointer width from
  `TargetInfo`.

## Acceptance
- On x86-64 Linux, `__x86_64__`, `__linux__`, `__LP64__` are all
  defined as `1`.
- `__SIZEOF_INT__` equals `4`, `__SIZEOF_POINTER__` equals `8` on
  LP64.
- Macros change correctly when targeting a different triple.

## References
- GCC predefined macros documentation.
- Clang `InitPreprocessor.cpp`.
