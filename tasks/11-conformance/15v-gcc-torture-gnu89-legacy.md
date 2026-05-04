# 11-15v: gcc-torture GNU89 legacy cases

> ✓ done — 2026-05-04

**Phase:** 11-conformance    **Depends on:** 11-15k    **Milestone:** M6

## Goal
Decide how gcc-torture GNU89-only runtime cases are represented in a C99-first
compiler project.

## Scope
- In: `920428-1`, `931018-1`.
- Out: C99 prototypes and modern declarations.

## Deliverables
- A short policy: unsupported, GNU89 mode task, or compatibility parser slice.
- If supported, tasks for implicit int, K&R function definitions, and legacy
  default argument behavior.

## Acceptance
- These cases no longer appear as unexplained runtime signal bugs.
- Any future xfail/skip is tied to this explicit policy task.

## References
- `docs/gcc-torture-signal-clusters.md`
