> ✓ done — 2026-05-05

# 13-03e: `-Wsign-compare`

**Phase:** 13-quality    **Depends on:** 13-03    **Milestone:** M7

## Goal
Warn when equality or relational comparisons mix signed and unsigned integer
types in a way that can surprise users.

## Scope
- In:
  - Use typeck conversion information after integer promotions/usual arithmetic
    conversions.
  - Warn under `-Wextra` or explicit `-Wsign-compare`.
  - Suppress with `-Wno-sign-compare`.
- Out:
  - `-Wconversion`.
  - Bit-precise range analysis.

## Deliverables
- Typeck warning and tests.
- Docs entry in `docs/warnings.md`.

## Acceptance
- `int i; unsigned u; return i < u;` warns under `-Wextra`.
- Same-signed comparisons do not warn.
- Explicit casts suppress the warning when they make the types match.
