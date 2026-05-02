> ✓ done — 2026-05-02

# 09-08: SysV ABI return lowering

**Phase:** 09-codegen-llvm    **Depends on:** 09-05, 09-06, 09-07    **Milestone:** M3

## Goal

Classify C return types and map them to LLVM return types, including hidden
`sret` pointers for memory-class aggregate returns.

## Scope

- In: void, scalar, pointer, small aggregate, large aggregate, and `_Bool`
  normalization.
- Out: C++-style ABI rules and non-SysV targets.

## Deliverables

- `AbiReturn` classification.
- Tests for direct return, indirect `sret`, and caller/callee agreement.

## Acceptance

- Function declaration and call emission use the same return classification.
- `return;` in `void` functions and scalar return values both verify in LLVM.

## References

- SysV x86-64 ABI 3.2.3
