> ✓ done — 2026-05-04

# 08-06: `if / else` lowering

**Phase:** 08-cfg    **Depends on:** 08-03    **Milestone:** M3

## Goal
Lower `HirStmtKind::If` to a `SwitchInt` terminator with two targets
(or a `Goto` pair when the branches trivially converge).

## Scope
- In: generate then / else / join blocks; omit join if both branches
  terminate (return / goto).
- Out: `switch` (task 08).

## Deliverables
- Lowering branch.
- Snapshot: `if (x) { a(); } else { b(); }`.

## Acceptance
- No unreachable `Goto` to join when both arms return.

## References
- C99 §6.8.4.1.
