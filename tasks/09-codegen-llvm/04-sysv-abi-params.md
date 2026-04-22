# 09-04: SysV ABI parameter classification

**Phase:** 09-codegen-llvm    **Depends on:** 09-02    **Milestone:** M3

## Goal
Classify each function parameter per System V x86-64 §3.2.3 into
one of `INTEGER`, `SSE`, `MEMORY`, `X87`, `X87UP`, `COMPLEX_X87`.
Determines whether a value is passed in registers or on the stack.

## Scope
- In: the classic "eightbytes" algorithm; emit `inkwell`-style
  attributes (`byval`) where needed.
- Out: non-SysV ABIs (Win64, ARM) — tracked but not implemented.

## Deliverables
- `classify_arg(ty: &Ty, tcx) -> Vec<ArgClass>`.
- Table fixture comparing classification to GCC's `-fcallgraph-info`
  output on a sample.

## Acceptance
- `struct { int a; int b; }` classified as one `INTEGER` eightbyte.
- `struct { double; double; }` classified as two `SSE` eightbytes.

## References
- System V ABI §3.2.3 "Parameter Passing".
