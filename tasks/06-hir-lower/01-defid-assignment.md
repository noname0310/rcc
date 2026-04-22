# 06-01: `DefId` assignment

**Phase:** 06-hir-lower    **Depends on:** —    **Milestone:** M2

## Goal
Walk the AST top-level and assign a `DefId` to every function
definition, global variable, typedef, and struct/union/enum tag.
The resulting `HirCrate::defs` is the spine for every other lowering
task.

## Scope
- In: pre-pass that populates `Resolver::ordinary` and `Resolver::tags`
  (placeholder entries only — actual `DefKind` filled later).
- Out: conflict detection (task 02).

## Deliverables
- First-pass AST walk.
- Regression test: every AST top-level produces exactly one `DefId`.

## Acceptance
- `HirCrate::defs.len()` equals (functions + globals + typedefs +
  tag definitions) at top level.

## References
- rustc's early `Definitions` table.
