# 06-15: finalize file-scope typedef and global definition types

**Phase:** 06-hir-lower    **Depends on:** 06-14    **Milestone:** M5 stabilization

## Goal
Fill the `DefKind::Typedef(tcx.error)` and
`DefKind::Global { ty: tcx.error, .. }` placeholders produced by
`assign_def_ids`.

## Scope
- In: a second pass after `assign_def_ids` that lowers every file-scope
  declaration's real type and writes it into the existing `DefId`.
- In: multiple declarators sharing one specifier list.
- In: tentative definitions and `extern` declarations.
- In: typedef chains and typedef cycle diagnostics using the existing
  `lower_typedef_name` cycle machinery.
- Out: static initializer code generation.

## Deliverables
- `lower()` calls a file-scope type finalization pass before function
  bodies are lowered.
- File-scope ordinary namespace entries have usable `TyId`s before
  expression lowering and typeck snapshots.
- Tests for globals, typedefs, arrays, pointers, and function
  declarations.

## Acceptance
- `typedef int T; T g;` gives `T` a `Typedef(tcx.int)` and `g` type
  `tcx.int`.
- `int *p, a[3];` stores different types for the two globals.
- `extern int x; int x;` reuses or reconciles the existing definition
  according to the existing linkage model instead of creating an
  unreachable `tcx.error` def.
- `typedef T U;` before `T` is complete emits a diagnostic or keeps
  `tcx.error` explicitly; it must not become `int`.

## References
- `assign_def_ids` currently creates file-scope placeholders only.
- `rcc_typeck::def_snapshot` assumes `Global` and `Typedef` carry real
  types.

