> ✓ done — 2026-05-04

# 11-15n1: gcc-torture pr77767 VLA parameter bound side effects

**Phase:** 11-conformance    **Depends on:** 11-15n    **Milestone:** M6

## Goal
Fix the remaining VLA parameter-bound side-effect bug behind
`gcc-torture::execute::pr77767`.

## Scope
- In: function-definition parameters such as `int b[a++]` and `int d[c++]`
  where the array parameter adjusts to pointer type but the bound expression
  still has runtime side effects at function entry.
- In: preserving source-order evaluation of multiple VLA parameter bounds.
- Out: non-parameter VLA allocation/deallocation; that is covered by 11-15n.

## Deliverables
- A reduced fixture equivalent to:
  `void foo(int a, int b[a++], int c, int d[c++])`.
- HIR/lowering support for emitting parameter-bound side-effect statements
  before the user function body.
- A WSL gcc-torture probe proving `pr77767` passes.

## Acceptance
- `gcc-torture::execute::pr77767` passes under WSL LLVM.
- The reduced fixture passes host `cc` and rcc.
- The HIR/MIR dump for `foo` shows the `a++` and `c++` side effects before
  the body condition.
- No xfail, skip, or result masking is added.

## Result
- Function-definition parameter lowering now emits non-constant array
  parameter bound expressions as entry statements before the user body.
- The emitted statements preserve parameter declaration order and resolve
  against already-declared parameters, so `int b[a++]` sees `a` before `b` is
  inserted.
- `gcc-torture::execute::pr77767` passes under WSL LLVM.
- A reduced driver e2e fixture and a HIR lowering regression test lock the
  behavior down without adding any xfail/skip/result masking.

## References
- `tasks/11-conformance/15n-gcc-torture-vla-lifetime-cluster.md`
- `docs/gcc-torture-signal-clusters.md`
