# 11-07: GNU statement-expression semantics

**Phase:** 11-conformance    **Depends on:** 11-05, 05-36    **Milestone:** M2+

## Goal
Lower and execute GNU statement expressions `({ ... })` instead of parsing
them and replacing them with a dummy integer constant.

## Scope
- In:
  - Replace the current `ExprKind::StmtExpr(_) -> HirExprKind::IntConst(0)`
    lowering fallback.
  - Add an HIR representation that preserves a block, the value of the final
    expression statement, and the `void` result case.
  - Type-check statement-expression blocks with normal block scope rules.
  - Lower statement expressions to CFG by executing the block and materializing
    the final value into a temporary.
  - Preserve labels/gotos inside the expression block when they do not escape
    the containing function.
- Out:
  - Computed goto / labels-as-values.
  - GNU case ranges.
  - Lifetime diagnostics beyond what is required to run chibicc stage tests.

## Deliverables
- HIR/typeck/CFG tests for:
  - `({ int i=2; i+=5; i; }) == 7`
  - `({ int i=2; i+=5; }) == 7`
  - nested blocks and local shadowing
  - `void` statement expression in expression-statement position
- One driver E2E fixture proving object code executes correctly.

## Acceptance
- `rcc` no longer silently compiles a statement expression as zero.
- `({ int i=2; i+=5; i; })` returns 7 in an executable test.
- The feature is still controlled by `Options::gnu_statement_expressions` for
  warning policy.

## References
- `docs/parser-feature-matrix.md`
- `crates/rcc_hir_lower/src/lib.rs`
- `crates/rcc_cfg/src/lower.rs`
