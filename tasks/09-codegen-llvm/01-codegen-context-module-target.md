> ??done ??2026-05-01

# 09-01: Codegen context, module, and target baseline

**Phase:** 09-codegen-llvm    **Depends on:** 08-cfg    **Milestone:** M3

## Goal

Create the backend foundation before any instruction emission: a
`CodegenCx` owning the inkwell `Context`, `Module`, `Builder`, target
triple, data layout string, HIR/CFG references, and diagnostic bridge.

## Scope

- In: `llvm` feature path only; Linux x86-64 SysV as the first target.
- In: module verifier helper and structured `CodegenError` conversion.
- Out: cross-target `rcc_target` crate; that remains phase 15.

## Deliverables

- `backend::CodegenCx` plus constructor used by `codegen_impl`.
- Deterministic module name and target triple/data-layout baseline.
- Tests for `BackendDisabled`, empty module verification, and non-empty IR text.

## Acceptance

- `codegen()` with `llvm` enabled returns textual LLVM IR containing a module
  header, target triple, and data layout.
- LLVM verifier failures become `CodegenError::Internal` with useful context.

## References

- `crates/rcc_codegen_llvm/src/lib.rs`
- LLVM LangRef: Module structure
