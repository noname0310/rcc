# 09-05: SysV ABI return handling

**Phase:** 09-codegen-llvm    **Depends on:** 09-04    **Milestone:** M3

## Goal
Handle return by value: scalar / small aggregate via registers (uses
classification from task 04); large aggregate via hidden pointer
(`sret` attribute).

## Scope
- In: decide ABI based on classified eightbytes; emit `sret` attribute
  on the prologue pointer when needed; caller reserves a stack slot
  and passes it as first arg.
- Out: variadic calling convention (task 13).

## Deliverables
- Return lowering branch.
- Fixture: returning `struct { double x, y; }` (two SSE) vs
  `struct { int a[3]; }` (sret).

## Acceptance
- LLVM IR for `struct S1 two_doubles()` has no `sret`; return type
  is `{ double, double }`.
- LLVM IR for `struct S2 big_struct()` has `sret` attribute on arg 0.

## References
- System V ABI §3.2.3 return handling.
