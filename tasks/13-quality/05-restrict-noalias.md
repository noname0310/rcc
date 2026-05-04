# 13-05: `restrict` -> LLVM `noalias`

**Phase:** 13-quality    **Depends on:** 06-26, 09-07, 09-13    **Milestone:** M7

## Goal
Lower the C99 `restrict` qualifier on pointer parameters to the
LLVM `noalias` attribute, enabling LLVM's alias analysis to
generate better optimised code.

## Scope
- In:
  - Verify parser and HIR lowering preserve `restrict` on pointer
    parameters.
  - Detect `restrict` qualifier on pointer-type function parameters during
    ABI lowering/codegen.
  - Emit the LLVM `noalias` attribute on the corresponding LLVM parameter
    unit, taking SysV aggregate/direct ABI splitting into account.
  - Document exactly which `restrict` forms are intentionally not optimized.
- Out:
  - `restrict` on local pointer variables (LLVM scoped noalias metadata on
    loads/stores).
  - Whole-program alias reasoning.

## Deliverables
- `restrict` detection in codegen parameter emission.
- LLVM `noalias` attribute on restrict-qualified pointer params.
- Test: verify LLVM IR contains `noalias` for `restrict` params.
- Negative tests for non-pointer and non-parameter restrict forms.

## Acceptance
- `void f(int * restrict p, int * restrict q)` emits LLVM IR
  with `noalias` on both pointer parameters.
- Non-restrict pointers do not get `noalias`.
- ABI-expanded parameters do not get attributes on the wrong LLVM argument.
- `cargo test -p rcc_codegen_llvm --features llvm --test llvm_filecheck`
  includes the positive and negative cases.

## References
- C99 §6.7.3.1 — Formal definition of `restrict`.
- LLVM `noalias` attribute documentation.
