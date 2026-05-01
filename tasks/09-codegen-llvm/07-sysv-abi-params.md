# 09-07: SysV ABI parameter lowering

**Phase:** 09-codegen-llvm    **Depends on:** 09-05, 09-06    **Milestone:** M3

## Goal

Classify function parameters according to the first supported ABI: System V
x86-64. This defines the LLVM function type and call-site argument lowering.

## Scope

- In: scalar, pointer, small aggregate, large aggregate by memory, and variadic
  fixed-parameter boundary.
- Out: Windows x64, i386, and full cross-target abstraction.

## Deliverables

- `AbiParam` classification data structure.
- Golden tests for representative scalar/aggregate signatures.

## Acceptance

- LLVM function types for simple C signatures match Clang's shape on SysV
  x86-64 for the tested subset.
- Large aggregate parameters are passed indirectly according to ABI policy.

## References

- SysV x86-64 ABI 3.2.3
