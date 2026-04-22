# 11-04: chibicc stages 1..3

**Phase:** 11-conformance    **Depends on:** 01-09, 06-12    **Milestone:** M2

## Goal
Pass the chibicc test files that correspond to compiler stages 1..3
in Rui Ueyama's progression: `arith.c`, `control.c`, `function.c`.

## Scope
- In: run via `ChibiccAdapter`; triage failures.
- Out: later-stage tests (`struct.c`, `sizeof.c`, etc.).

## Deliverables
- `xfail.toml` entries for later-stage files.
- KPI row updated.

## Acceptance
- Stages 1..3 all green in CI report.

## References
- chibicc `test/` numbering.
