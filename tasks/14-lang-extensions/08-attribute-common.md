> ✓ done — 2026-05-05

# 14-08: Common attribute semantics

**Phase:** 14-lang-extensions    **Depends on:** 14-07    **Milestone:** M6

## Goal
Implement semantic handling for the most commonly used GCC
attributes: `packed`, `aligned(N)`, `noreturn`, `unused`,
`deprecated`, `visibility("default"|"hidden")`, `section("name")`,
`weak`. Wire attribute effects into HIR `Def` nodes and LLVM
codegen.

## Scope
- In: attribute validation (correct number/type of arguments),
  propagation to HIR, codegen lowering:
  - `packed` → set struct layout to packed (alignment 1).
  - `aligned(N)` → override minimum alignment.
  - `noreturn` → LLVM `noreturn` attribute on function.
  - `unused` → suppress unused-variable warnings.
  - `deprecated` → emit warning on use.
  - `visibility` → LLVM `default`/`hidden` visibility.
  - `section` → LLVM section attribute on global/function.
  - `weak` → LLVM `weak` linkage.
- Out: lesser-used attributes (`format`, `constructor`,
  `destructor`, `alias` — future tasks).

## Deliverables
- Attribute semantic checker in `rcc_hir_lower` or `rcc_typeck`.
- Codegen wiring in `rcc_codegen_llvm`.
- Tests for each attribute's effect.

## Acceptance
- `struct __attribute__((packed)) S { ... }` has no padding.
- `__attribute__((noreturn)) void f()` produces LLVM IR with
  `noreturn` attribute.
- Using a `deprecated` symbol emits a warning.

## References
- GCC common function/variable/type attributes documentation.
