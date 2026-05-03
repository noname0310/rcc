> ✓ done — 2026-05-04

# 08-13: Variable-length array lowering

> **Status:** done. Block-scope VLA declarations now preserve and
> evaluate their runtime bound, store it in CFG metadata for the local,
> and lower `sizeof(vla)` through `Rvalue::Len(place)` multiplied by the
> element size. `StorageDead` continues to delimit VLA lifetimes.

**Phase:** 08-cfg    **Depends on:** 08-02, 08-12    **Milestone:** M6

## Goal
Support C99 VLAs. At the declaration, evaluate the size expression at
runtime and emit a dynamic `alloca`. Indexing uses the computed size
for bounds-checked addressing (we don't bounds-check, but the size is
needed for `sizeof(vla)`).

## Scope
- In: `Rvalue::Len(Place)` that codegen maps to a saved runtime size
  local; `StorageDead` must deallocate (LLVM's alloca doesn't free —
  codegen uses `llvm.stackrestore`).
- Out: runtime bounds checking (not in C99; separate sanitiser).

## Deliverables
- VLA lowering branch in local-decl handler.
- Snapshot: `void f(int n) { int a[n]; }`.

## Acceptance
- `sizeof(a)` inside `f` returns `n * sizeof(int)` at runtime (test
  via differential vs host cc).
- Two VLAs in sequence don't leak stack across scopes.

## References
- C99 §6.7.5.2.
