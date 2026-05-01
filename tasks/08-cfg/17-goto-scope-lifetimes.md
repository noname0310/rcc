> ✓ done — 2026-05-01

# 08-17: Goto scope lifetime fixup

**Phase:** 08-cfg    **Depends on:** 08-16    **Milestone:** M6 stabilization

## Goal
Make `goto` paths preserve the same storage lifetime invariants as
fallthrough, `break`, `continue`, and `return`. Today label collection
stores only a block id, so a goto that leaves nested scopes cannot emit
the required `StorageDead` statements.

## Scope
- In: enrich label metadata from `Symbol -> BasicBlockId` to a label
  info record that includes lexical scope depth and VLA-scope depth.
- In: make `collect_labels` track lexical depth while scanning HIR.
- In: when a `goto` jumps outward, emit `StorageDead` for every local
  whose scope is exited.
- In: reject or surface a structured CFG/HIR error for jumps into a
  scope that would bypass initialized locals or VLA allocation.
- Out: non-local jumps (`setjmp`/`longjmp`), which are library/runtime
  semantics.

## Deliverables
- `LabelInfo` metadata in `BodyBuilder`.
- `goto` lowering that calls the same lifetime-exit helper used by
  `break`/`continue`/`return`.
- Tests for goto out of a block, goto out of nested blocks, goto across
  a VLA declaration, and a forward label inside a switch/case body.
- A verifier assertion that every named local live on a reachable path
  has a matching dead marker before the path exits its scope.

## Acceptance
- `int f(int x) { { int y = 1; if (x) goto L; } L: return 0; }`
  emits `StorageDead(y)` on the goto path.
- A goto into a VLA scope is not silently lowered as valid CFG.
- Existing switch/goto snapshots remain stable except for intentional
  lifetime marker additions.

## References
- C99 §6.8.6.1 `goto`.
- `crates/rcc_cfg/src/build.rs` `label_map` and `collect_labels`.
- `crates/rcc_cfg/src/lower.rs` current `HirStmtKind::Goto` comment.
