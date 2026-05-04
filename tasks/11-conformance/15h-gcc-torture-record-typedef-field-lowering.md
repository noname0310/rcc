# 11-15h: gcc-torture record typedef field lowering

> ✓ done — 2026-05-04

**Phase:** 11-conformance    **Depends on:** 11-15b    **Milestone:** M6

## Goal
Fix the HIR/typeck record-field lowering bug exposed after `__builtin_printf`
aliases allow `gcc-torture::execute::20020406-1` to progress past name
resolution.

## Scope
- In: file-scope typedef names used in record fields, especially
  `typedef unsigned int FFelem; struct S { FFelem *coeffs; };`.
- In: typed-HIR verification failures where a record field contains
  `Ty::Error` despite a valid typedef being in scope.
- Out: unrelated runtime failures in the same fixture after the record type is
  fixed.

## Deliverables
- A reduced HIR-lower/typeck regression for typedef-based record pointer
  fields.
- A fix ensuring valid typedef field types never reach typed-HIR verify as
  `Ty::Error`.
- A representative gcc-torture rerun for `20020406-1`.

## Acceptance
- `struct S { T *p; };` resolves `T` through the ordinary identifier namespace
  when `T` is a visible typedef.
- `gcc-torture::execute::20020406-1` no longer fails with
  `record ... field ... type contains Ty::Error`.
- No xfail/skip is added.

## References
- `target/wsl/gcc-torture-15b-builtin-probes.json`

## Result
- Added a file-scope typedef finalization prepass before record tag
  materialization so record field specifiers see real typedef types rather
  than `Ty::Error` placeholders.
- Added a regression that locks `typedef unsigned int FFelem; struct S {
  FFelem *coeffs; };` to an `unsigned int *` field type.
- WSL representative gcc-torture rerun:
  `gcc-torture::execute::20020406-1` now passes.
