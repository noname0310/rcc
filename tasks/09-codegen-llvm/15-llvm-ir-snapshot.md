# 09-15: `--emit=llvm-ir` snapshot tests

**Phase:** 09-codegen-llvm    **Depends on:** 09-06    **Milestone:** M3

## Goal
Use `insta` to lock down LLVM-IR dumps for a dozen representative
fixtures. Changes in codegen should require a reviewed snapshot
update.

## Scope
- In: feature-gated tests (`#[cfg(feature = "llvm")]`); normalise
  IR by replacing pointer addresses with `X` placeholders; pre-dump
  through `opt -mem2reg,instcombine` for stability.
- Out: FileCheck tests (task 16).

## Deliverables
- ≥ 12 fixtures.
- `tests/snapshots/llvm_ir/`.

## Acceptance
- `cargo test -p rcc_codegen_llvm --features llvm --test snapshot`:
  green (local LLVM install required).

## References
- `insta` + LLVM IR normalisation tricks.
