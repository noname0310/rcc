# 04-17: Preprocessor unit test matrix

**Phase:** 04-preprocess    **Depends on:** 04-06 .. 04-14    **Milestone:** M5

## Goal
A single `crates/rcc_preprocess/tests/expansion.rs` file driven by a
`&[(&str, &str)]` table of `(source, expanded)` pairs, covering every
feature added in this phase. Golden for regressions.

## Scope
- In: ≥ 40 rows covering object-like, function-like, `#`, `##`,
  variadic, recursive guards, nested `#if`, `#line`, predefined macros.
- Out: include-path resolution (requires fs fixtures — separate test).

## Deliverables
- `expansion.rs` with a `run_table()` helper.

## Acceptance
- `cargo test -p rcc_preprocess --test expansion`: green.
- Rows ported from chibicc's `test/macro.c` (subset) verbatim.

## References
- chibicc `test/macro.c`.
