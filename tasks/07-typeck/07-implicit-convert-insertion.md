# 07-07: Wire implicit conversions into HIR

**Phase:** 07-typeck    **Depends on:** 07-01 .. 07-06    **Milestone:** M3

## Goal
Produce the final mutated `HirCrate` where every `HirExpr` has a real
`TyId` and every required conversion is materialised as a
`HirExprKind::Convert` node. The CFG builder then never re-derives
these rules.

## Scope
- In: top-down + bottom-up combined pass: type each node, then, at
  each operator, wrap operands as needed.
- Out: propagating `restrict` analysis (M7).

## Deliverables
- `check_body(body, tcx, session)` fn finalising every expression.
- Snapshot tests: HIR pretty-print before vs after typeck.

## Acceptance
- `1 + 2.0` — IntConst is wrapped in `Convert(IntToFloat, f64)`
  before the `FAdd`.
- No `Ty::Error` surfaces in a clean HIR after typeck.

## References
- All of C99 §6.3 + §6.5.
