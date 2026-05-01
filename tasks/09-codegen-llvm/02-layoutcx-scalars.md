# 09-02: LayoutCx scalar layout

**Phase:** 09-codegen-llvm    **Depends on:** 09-01    **Milestone:** M3

## Goal

Finalize backend-visible size, alignment, and ABI alignment for every scalar
`Ty`: integer ranks, floating ranks, pointers, enums, and `_Bool`.

## Scope

- In: LP64 / SysV x86-64 baseline values matching the module data layout.
- In: explicit tests for signed/unsigned ranks sharing size/align.
- Out: target abstraction beyond the baseline.

## Deliverables

- Exhaustive scalar layout table tests.
- Assertion that `LayoutCx` pointer size matches the LLVM data layout baseline.

## Acceptance

- `sizeof(char/short/int/long/long long/_Bool/void*)` agrees with host cc on
  x86-64 Linux fixtures.
- `LayoutCx` never returns `Ty::Error` as a valid layout.

## References

- C99 6.2.5
- SysV x86-64 ABI data representation
