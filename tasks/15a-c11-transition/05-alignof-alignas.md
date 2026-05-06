> ✓ done — 2026-05-06

# 15a-05: `_Alignof`, `_Alignas`, and `stdalign.h`

**Phase:** 15a-c11-transition  
**Depends on:** 15a-04-static-assert-declarations  
**Milestone:** c11-transition

## Goal

Implement C11 alignment query and alignment specifier support in a way that
feeds the existing layout service instead of duplicating target-layout logic.

## Scope

- In: parse `_Alignof(type-name)` as a unary expression.
- In: parse `_Alignas(type-name)` and `_Alignas(constant-expression)` as
  alignment specifiers in declaration-specifier lists.
- In: lower requested alignment onto HIR declarations/fields where applicable.
- In: reject invalid alignments with C11 constraint diagnostics.
- In: add `<stdalign.h>` macros `alignas` and `alignof`.
- Out: C++ `alignas` grammar or C23 attribute spelling.

## Acceptance

- [x] `_Alignof(int)` folds to the target ABI alignment.
- [x] `_Alignas(16) int x;` affects object alignment in HIR/layout metadata.
- [x] Invalid alignments such as `_Alignas(3)` are diagnosed.
- [x] `#include <stdalign.h>` exposes `alignas` and `alignof` in C11 mode.
- [x] No layout regression for existing structs, arrays, and bitfields.

## References

- N1570 6.5.3.4 `sizeof` and `_Alignof`.
- N1570 6.7.5 alignment specifier.
- N1570 7.15 `stdalign.h`.
