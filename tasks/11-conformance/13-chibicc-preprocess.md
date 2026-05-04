> ✓ done — 2026-05-04

# 11-13: chibicc preprocessor tests

**Phase:** 11-conformance    **Depends on:** 04-18    **Milestone:** M5

## Goal
All preprocessor-focused chibicc fixtures (`macro.c`, `include.c`,
`typedef.c`) pass. Landmark for M5 completion.

## Scope
- In: run via `ChibiccAdapter::run_preprocess_only`; fix anything
  still failing.
- Out: full M5 integration (other tests still running).

## Deliverables
- Green KPI cell.

## Acceptance
- `rcc --emit=pp chibicc-fixture.c` matches `cc -E` on each file.

## Notes (agent)
- Wired CLI flags for the already-implemented GNU preprocessor compatibility
  switches: permissive macro redefinition, named variadic macros, permissive
  token paste, and comma elision for empty `__VA_ARGS__`.
- `ChibiccAdapter::run_preprocess_only` now enables those flags explicitly.
- The vendored chibicc snapshot discovers `macro.c` and `typedef.c` for
  preprocess mode; `include.c` is listed as optional/reserved and is not present
  in this checkout.
- WSL preprocess report: `macro` and `typedef` both pass.

## References
- Plan §10 M5.
