# 06-02: Ordinary name resolution

**Phase:** 06-hir-lower    **Depends on:** 06-01    **Milestone:** M2

## Goal
Resolve every `ExprKind::Ident` to either a `LocalRef` or a `DefRef`,
following C99 §6.2.1 scope rules. Redeclarations with conflicting
linkage or type → E0070.

## Scope
- In: a per-body scope stack that shadows parameters with locals
  shadowed with inner-block locals; file-scope lookup falls through
  to the resolver built in task 01.
- Out: tag namespace (task 03) and labels (task 04).

## Deliverables
- `resolve_expr_ident(ident, scope) -> HirExprKind`.
- Fixture with shadowing, re-declaration, use-before-declaration.

## Acceptance
- `int x; void f() { int x = 0; x = 1; }` — both `x` references
  resolve to the function-scope local.
- Using an undeclared identifier → E0071 with a `help:` suggesting
  similar-named symbols.

## References
- C99 §6.2.1, §6.2.2.
