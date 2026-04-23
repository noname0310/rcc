> ✓ done — 2026-04-23

# 02-02: Error-codes registry

**Phase:** 02-diagnostics    **Depends on:** 02-01    **Milestone:** M1

## Goal
Create a stable `ErrorCode`-per-diagnostic scheme so every user-visible
message can be looked up in `docs/error-codes.md`. Follows rustc's
`E0XXX` convention.

## Scope
- In: `crates/rcc_errors/src/codes.rs` with a `pub const EXXXX: &str = "...";`
  list; a build-time check that every code used in the workspace
  appears in `docs/error-codes.md`.
- Out: specific diagnostic authoring (that's phase 03..07 work; each
  feature task adds its own codes).

## Deliverables
- `codes.rs` with initial ~20 codes reserved for lexer/preprocess
  (`E0001..E0020`).
- `docs/error-codes.md` auto-generated per code with title + example.
- `xtask check-error-codes` sub-command that cross-references
  occurrences in source vs docs.

## Acceptance
- `cargo xtask check-error-codes` green.
- CI fails when a new `E0XXX` is introduced without an entry in
  `docs/error-codes.md`.

## References
- rustc book's error index layout.
