> ✓ done — 2026-05-04

# 08-10: Call lowering

**Phase:** 08-cfg    **Depends on:** 08-03    **Milestone:** M3

## Goal
`HirExprKind::Call` lowers to a `TerminatorKind::Call` splitting the
current block. Arguments are reduced to `Operand`s; the destination
is either a new temporary `Place` or `None` for a void call.

## Scope
- In: evaluate args left-to-right, pushing `Assign` to temps as
  needed; for variadic callees, argument rvalues are passed through
  as-is (ABI classification is codegen's job).
- Out: --.

## Deliverables
- Call lowering helper.
- Snapshot: `printf("%d\\n", x);`.

## Acceptance
- The successor block receives the return value at its entry.
- `exit(1)` — no target block (Call is a non-returning call form if
  callee is marked `_Noreturn`; otherwise normal form).

## References
- C99 §6.5.2.2.
