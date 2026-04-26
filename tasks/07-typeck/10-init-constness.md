> ✓ done — 2026-04-27

# 07-10: Global initializer constness

**Phase:** 07-typeck    **Depends on:** 07-09    **Milestone:** M4

## Goal
For every global / static declaration with an initializer, verify the
initializer is a constant expression per §6.7.8p4. Non-constant
→ E0084 with a label on the offending sub-expression.

## Scope
- In: recursive descent through `Initializer` + `HirExpr` using
  `ConstEval` to prove constness; aggregate initializers need each
  leaf value to be constant.
- Out: local variable initialisers (those are arbitrary expressions).

## Deliverables
- `check_init_const(def: &Def, init: &Initializer, tcx)`.
- Fixtures: `static int x = 2 + 3;` OK; `static int x = foo();` error.

## Acceptance
- All c-testsuite files with globals pass the check.
- Non-const global trips E0084 with helpful label.

## References
- C99 §6.7.8p4.
