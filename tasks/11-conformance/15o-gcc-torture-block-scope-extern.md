> ✓ done — 2026-05-04

# 11-15o: gcc-torture block-scope extern resolution

**Phase:** 11-conformance    **Depends on:** 11-15k    **Milestone:** M6

## Goal
Fix C99 name resolution for block-scope `extern` declarations that should bind
to file-scope objects instead of shadowing block locals.

## Scope
- In: `scope-1` and reduced block-scope `extern int v;` fixtures.
- Out: tentative definition merging unrelated to block-scope externs.

## Deliverables
- Resolver tests proving an inner `extern` finds the visible file-scope
  declaration even when a block local has the same name.
- HIR/CFG/codegen regression proving the loaded object is the global.

## Acceptance
- `gcc-torture::execute::scope-1` passes.
- The reduced fixture in `docs/gcc-torture-signal-clusters.md` passes.

## Result
- Block-scope `extern` object declarations now bind the current block scope to
  an existing file-scope `DefKind::Global` when one is visible through the
  resolver's file-scope ordinary table.
- If no file-scope global exists, lowering creates an external-linkage global
  `Def` and binds only the block scope to it, avoiding accidental file-scope
  visibility.
- `gcc-torture::execute::scope-1` and a reduced driver e2e fixture pass under
  WSL LLVM.

## References
- `docs/gcc-torture-signal-clusters.md`
