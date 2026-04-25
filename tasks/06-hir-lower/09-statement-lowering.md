> ✓ done — 2026-04-25

# 06-09: Statement lowering

**Phase:** 06-hir-lower    **Depends on:** 06-02, 06-04    **Milestone:** M2

## Goal
Map every `rcc_ast::StmtKind` variant to a `rcc_hir::HirStmtKind`.
Local variables encountered along the way populate `Body::locals`;
`HirStmtKind::LocalDecl { local, init }` records the binding.

## Scope
- In: scope push on blocks / for-init; pop on exit; labels resolved
  (task 04); for-init declarations create locals with proper
  lifetimes.
- Out: evaluating initialisers (task 11) is orchestrated here but
  delegated to that task.

## Deliverables
- `lower_stmt(ast, body, scope) -> HirStmtId`.
- Tests for each statement kind.

## Acceptance
- AST `for (int i = 0; i < n; ++i) body` lowers to a `For` with
  `init = LocalDecl { local: i, init: 0 }`.

## References
- C99 §6.8.
