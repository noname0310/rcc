> ✓ done — 2026-05-01

# 05-37: GNU range designator syntax

**Phase:** 05-parse    **Depends on:** 05-36    **Milestone:** M5 blocker

## Goal
Parse GNU initializer range designators such as `[1 ... 5] = expr`
without forcing initializer lowering to infer ranges from ordinary
index designators.

## Scope
- In:
  - Extend AST designators with a range form carrying start and end
    assignment-expressions.
  - Extend `parse_designator_chain` to accept `[lo ... hi]`.
  - Add diagnostics for malformed ranges, missing `]`, missing `=`,
    or reversed constant ranges if parser-level checking is chosen.
  - Add tests from reduced c-testsuite `00216` fixtures.
  - Gate under GNU extension mode.
- Out:
  - Constant evaluation of range bounds.
  - Expansion of ranges into per-element initializers.
  - Overlap resolution semantics.

## Deliverables
- AST `Designator::Range` or equivalent.
- Parser tests for scalar, array, and nested range designators.
- HIR-lower follow-up note if range expansion remains deferred.

## Notes
- Parser work is complete: `[lo ... hi]` is preserved as
  `Designator::Range` and strict C99 mode emits W0014.
- Range expansion remains intentionally deferred to
  `tasks/06-hir-lower/24-gnu-range-designator-lowering.md`.

## Acceptance
- `int a[8] = { [1 ... 5] = 9 };` parses in extension mode.
- Strict C99 mode produces a clear parser diagnostic or extension
  warning according to policy.
- Existing C99 designators `[0]`, `.field`, and `.field[1]` remain
  unchanged.

## References
- GNU C designated initializer ranges.
- `third_party/testsuites/c-testsuite/tests/single-exec/00216.c`.
- `crates/rcc_parse/src/init.rs`.
