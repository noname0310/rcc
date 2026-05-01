# 06-16: real block-scope typedef definitions

> ✓ done — 2026-05-01

**Phase:** 06-hir-lower    **Depends on:** 06-15    **Milestone:** M5 stabilization

## Goal
Make block-scope typedefs first-class HIR definitions instead of silently
skipping them after parser-level name recognition.

## Scope
- In: create `DefKind::Typedef(ty)` for block-scope typedef
  declarations.
- In: bind those defs in `ScopeStack` with `Binding::Def`.
- In: shadowing between locals, block typedefs, and file-scope typedefs.
- In: typedef use in later declarators and expressions within the same
  block.
- Out: debug info for block typedefs.

## Deliverables
- `lower_block_decl` handles `StorageClass::Typedef` by creating scoped
  HIR defs.
- `lower_typedef_name` can resolve a typedef from local scope, not only
  `resolver.ordinary`.
- Tests for nested typedef scopes and shadowing.

## Acceptance
- `void f(void) { typedef long T; T x; }` lowers `x` as `long`.
- `typedef int T; void f(void) { typedef long T; T x; }` uses the inner
  `T` for `x`.
- A block-scope object named `T` can shadow an outer typedef in ordinary
  namespace contexts exactly as C requires.
- No runtime `HirStmtKind` is emitted for a typedef declaration.

## References
- C99 §6.2.1 — Scopes of identifiers.
- `lower_block_decl` currently says block typedef handling is deferred.
