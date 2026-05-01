# 07-20: Object qualifier constraints

**Phase:** 07-typeck    **Depends on:** 06-26, 07-19    **Milestone:** M5

## Goal
Use the qualifier metadata preserved by 06-26 to enforce C99
modifiable-lvalue rules and to mark memory accesses that LLVM codegen
must treat as volatile.

## Scope
- In: reject assignments, increments, decrements, and compound
  assignments to `const`-qualified objects.
- In: preserve volatile information on lvalue expressions so CFG or
  codegen can identify volatile loads/stores.
- In: keep pointer-pointee qualifier conversion checks from 07-06
  intact.
- Out: LLVM `load volatile` / `store volatile`; owned by 09-18.

## Deliverables
- Typeck helper for object-qualified lvalues.
- Diagnostics for assignment to const-qualified object.
- Tests for direct locals/globals, fields, dereferenced pointers, and
  arrays where applicable.

## Acceptance
- `const int x = 1; x = 2;` emits a modifiable-lvalue diagnostic.
- `struct S { const int x; }; s.x = 1;` emits the same diagnostic.
- `volatile int x; int y = x; x = y;` reaches CFG/codegen with volatile
  access metadata preserved.

## References
- C99 §6.3.2.1p1, §6.5.16p2, §6.7.3.
- 06-26.
- 09-18.
