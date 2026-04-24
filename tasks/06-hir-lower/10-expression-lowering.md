> ✓ done — 2026-04-25

# 06-10: Expression lowering

**Phase:** 06-hir-lower    **Depends on:** 06-02    **Milestone:** M2

## Goal
Map every `cc_ast::ExprKind` variant to `cc_hir::HirExprKind`, resolving
identifier references. Types are still placeholders (`TyCtxt::error`);
the typeck phase fills them in.

## Scope
- In: traversal + resolution; string-literals interned into globals
  (task 04-12 predefined is separate — string literal → auto-
  generated `DefId::Global` with array-of-char type).
- Out: type-checking (phase 07).

## Deliverables
- `lower_expr(ast, body, tcx, resolver) -> HirExprId`.
- Tests using `Session::for_test`; assert AST → HIR node count is
  preserved.

## Acceptance
- All AST expression kinds reachable in HIR.
- String literal `"hi"` appears as `HirExprKind::StringRef(def_id)`
  referring to a new `DefKind::Global { ty: [char; 3], linkage: Internal, ... }`.

## References
- C99 §6.5.
