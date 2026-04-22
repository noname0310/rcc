# 09-16: FileCheck-style tests

**Phase:** 09-codegen-llvm    **Depends on:** 09-15    **Milestone:** M3

## Goal
Add a lightweight `// CHECK:` / `// CHECK-NEXT:` matcher (LLVM's
FileCheck-lite) to assert specific LLVM instructions appear in the
generated IR for a given source file.

## Scope
- In: new test runner in `crates/rcc_codegen_llvm/tests/filecheck.rs`;
  walks `.c` fixtures; extracts `CHECK:` lines; runs against
  `CodegenArtifact::ir_text`.
- Out: exact FileCheck dialect (we only need CHECK / CHECK-NEXT /
  CHECK-NOT for now).

## Deliverables
- Matcher.
- ≥ 20 `.c` fixtures per feature area (binop, cast, call, struct).

## Acceptance
- Target fixture with `// CHECK: add nsw i32` matches the real IR.

## References
- LLVM FileCheck.
