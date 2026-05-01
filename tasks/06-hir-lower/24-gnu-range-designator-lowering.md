# 06-24: GNU range designator lowering

> ✓ done — 2026-05-01

**Phase:** 06-hir-lower    **Depends on:** 05-37    **Milestone:** M5 blocker

## Goal
Lower GNU initializer range designators `[lo ... hi] = value` without
silently losing the range semantics that the parser now preserves.

## Scope
- In:
  - Expand `Designator::Range { lo, hi }` for array initializers into
    per-element assignments in both local and global initializer paths.
  - Evaluate `lo`/`hi` with the existing constant-expression helper and
    emit `E0079` for non-constant, reversed, or out-of-bounds ranges.
  - Preserve GNU overlap semantics: later initializer entries override
    earlier entries in source order.
  - Teach incomplete-array completion to use the largest expanded range
    upper bound.
  - Add HIR lowering tests for local arrays, nested array fields, globals,
    overlap, reversed ranges, and non-constant bounds.
- Out:
  - Anonymous aggregate member lookup.
  - Type-checking of the initializer value expression.
  - Codegen of the final global initializer representation.

## Deliverables
- `rcc_hir_lower` range expansion for local and global initializers.
- Unit tests covering the expansion and diagnostic paths.
- `third_party/testsuites/c-testsuite/xfail.toml` update if `00216`
  still fails only for anonymous aggregate or header reasons afterward.

## Acceptance
- `int a[8] = { [1 ... 5] = 9 };` lowers to writes for indices 1..=5.
- `{ [1 ... 3] = 1, [2] = 9 }` leaves index 2 with the later value.
- Reversed or non-constant bounds emit `E0079` and do not panic.
- Existing C99 `[N]`, `.field`, and `.field[N]` initializer lowering
  tests remain unchanged.

## Result
- Local array initializers expand `Designator::Range` into one write per
  selected index, preserving source order so later initializers override
  earlier writes.
- Global array initializers emit one `GlobalInitEntry` per expanded range
  element, again preserving source order.
- Incomplete array completion already uses the range upper bound and is
  covered by a source-pipeline regression test.
- Invalid range bounds (non-constant, reversed, negative, or out of
  declared bounds) emit `E0079`.
- `c-testsuite::00216` no longer names GNU range lowering as a blocker;
  its remaining parse xfail owners are extension syntax and freestanding
  headers.

## References
- `crates/rcc_ast/src/lib.rs` `Designator::Range`.
- `crates/rcc_hir_lower/src/lib.rs` initializer walkers.
- `third_party/testsuites/c-testsuite/tests/single-exec/00216.c`.
