# 09-03: Array + flexible array member layout

**Phase:** 09-codegen-llvm    **Depends on:** 09-02    **Milestone:** M4

## Goal
Compute layout for `Ty::Array { elem, len, is_vla }`. Fixed-length:
`len * elem_size`, alignment = elem align. Incomplete (`len = None`)
at top of a declaration = external; as the last field of a struct =
flexible array member, size 0, align = elem align.

## Scope
- In: handle `is_vla = true` as `size = 0, align = elem.align` (runtime
  size computed per-local).
- Out: VLA codegen path (task 13).

## Deliverables
- Array branch of `layout_of`.
- Tests: `int[10]`, `int[]` file-scope, flexible array in struct.

## Acceptance
- `struct { int a; char data[]; }`: size 4, align 4; `data` offset 4.

## References
- C99 §6.7.2.1p16.
