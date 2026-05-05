> ✓ done — 2026-05-05

# 13-03c: `-Wunused-parameter`

**Phase:** 13-quality    **Depends on:** 13-03    **Milestone:** M7

## Goal
Warn for function parameters that are never read when `-Wextra` or
`-Wunused-parameter` is enabled.

## Scope
- In:
  - Track parameter locals and reads inside function bodies.
  - Suppress the warning for unnamed parameters and explicit `(void)param`
    casts if the AST/HIR represents them.
  - Suppress with `-Wno-unused-parameter`.
- Out:
  - Interprocedural analysis.

## Deliverables
- Detector pass and tests.
- Docs entry in `docs/warnings.md`.

## Acceptance
- `int f(int x) { return 0; }` warns under `-Wextra`.
- `int f(int x) { return x; }` does not warn.
- `-Wall` alone does not enable this warning unless explicitly documented.
