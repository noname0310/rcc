> ✓ done — 2026-05-04

# 10-04: `insta` snapshot harness

**Phase:** 10-driver    **Depends on:** 10-01    **Milestone:** M3

## Goal
Centralise the `insta` setup across crates so every `--emit` stage
snapshot lives under `crates/rcc_driver/tests/snapshots/<stage>/`.

## Scope
- In: macro `assert_emit_snapshot!(path, stage)` run against a
  checked-in `.c` fixture; use `insta` settings to redact volatile
  bytes (paths, line numbers where appropriate).
- Out: HTML diff UI (insta-cli handles it).

## Deliverables
- Shared helper crate `rcc_test_support` (optional) or a dev-only
  module under `rcc_driver`.
- Snapshots for each stage on a common `hello.c` fixture.

## Acceptance
- `cargo insta review` flows cleanly.
- Snapshots stable across reruns on the same commit.

## References
- insta docs.
