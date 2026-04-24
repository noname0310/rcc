# 13-06: `restrict` → LLVM `noalias`

**Phase:** 13-quality    **Depends on:** 09-04    **Milestone:** M7

## Goal
Lower the C99 `restrict` qualifier on pointer parameters to the
LLVM `noalias` attribute, enabling LLVM's alias analysis to
generate better optimised code.

## Scope
- In: detect `restrict` qualifier on pointer-type function
  parameters during codegen. Emit the `noalias` attribute on the
  corresponding LLVM function parameter. Only applies to function
  parameters (C99 §6.7.3.1).
- Out: `restrict` on local pointer variables (LLVM `noalias`
  metadata on loads — more complex, defer).

## Deliverables
- `restrict` detection in codegen parameter emission.
- LLVM `noalias` attribute on restrict-qualified pointer params.
- Test: verify LLVM IR contains `noalias` for `restrict` params.

## Acceptance
- `void f(int * restrict p, int * restrict q)` emits LLVM IR
  with `noalias` on both pointer parameters.
- Non-restrict pointers do not get `noalias`.
- `opt -O2` can exploit `noalias` to vectorise a loop that would
  otherwise be blocked by aliasing.

## References
- C99 §6.7.3.1 — Formal definition of `restrict`.
- LLVM `noalias` attribute documentation.
