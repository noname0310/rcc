# 09-24: FileCheck-style tests

**Phase:** 09-codegen-llvm    **Depends on:** 09-23    **Milestone:** M4

## Goal

Add LLVM-style semantic IR tests for properties that snapshots express poorly,
such as instruction presence, linkage, ABI attributes, and volatile operations.

## Scope

- In: lightweight `// CHECK:` runner, normalized IR input, negative checks, and
  CI integration for `llvm` feature environments.
- Out: replacing all snapshots.

## Deliverables

- `FileCheck-lite` helper or documented external FileCheck integration.
- Tests for sret, internal linkage, mem intrinsics, volatile, and bitfield masks.

## Acceptance

- A missing expected instruction fails the test with a useful diff.
- Tests skip cleanly when LLVM tools are unavailable in non-LLVM CI jobs.

## References

- LLVM FileCheck documentation
