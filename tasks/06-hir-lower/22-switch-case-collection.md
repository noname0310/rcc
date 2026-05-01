> ✓ done — 2026-05-01

# 06-22: populate switch case tables from real source

**Phase:** 06-hir-lower    **Depends on:** 06-21    **Milestone:** M5 stabilization

## Goal
Ensure real-source `switch` statements lower with populated
`Vec<SwitchCase>` tables so CFG lowering does not need hand-built tests
to see switch targets.

## Scope
- In: collect `case` and `default` statements under each switch.
- In: nested switch isolation.
- In: duplicate case/default diagnostics or explicit typeck handoff.
- In: case expression folding through the const-eval path available at
  this phase.
- Out: jump-table optimization.

## Deliverables
- A switch-case collection pass in HIR lowering or immediately after
  typeck, with ownership documented.
- `HirStmtKind::Switch { cases }` is non-empty for source switches that
  contain labels.
- Tests that start from parsed source, not hand-built HIR.

## Acceptance
- `switch (x) { case 1: return 2; default: return 3; }` reaches CFG
  with two switch cases.
- Nested switches do not leak inner cases into the outer switch.
- A `case` outside any switch emits a diagnostic.
- Duplicate `default` labels emit a diagnostic.

## References
- C99 §6.8.4.2 — The `switch` statement.
- CFG switch tests currently hand-build `SwitchCase`; real HIR lowering
  leaves `cases: Vec::new()`.
