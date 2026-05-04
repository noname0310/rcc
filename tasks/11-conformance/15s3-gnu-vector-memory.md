# 11-15s3: GNU vector memory loads, stores, and byte views

**Phase:** 11-conformance    **Depends on:** 11-15s2    **Milestone:** M6

## Goal
Make vector objects load, store, copy, and alias through pointer/union byte views
using their vector layout.

## Scope
- In: vector lvalue load/store.
- In: pointer casts such as `*(__m128i *)&b[0] = c`.
- In: union byte views used by gcc-torture.
- Out: may-alias semantics beyond preserving the explicit vector store.

## Deliverables
- CFG place/load/store support for `Ty::Vector`.
- LLVM load/store generation for vector objects.
- Runtime fixture for vector store observed through scalar array/`memcmp`.

## Acceptance
- `simd-6` reduced fixture passes.
- `pr92618` pointer-store reduction writes the correct backing array bytes.
- Vector object copies do not go through aggregate record layout.

## References
- `docs/gnu-vector-design.md`
- `gcc-torture::execute::simd-6`
- `gcc-torture::execute::pr92618`
