# 01-10: Write `docs/conformance.json`

**Phase:** 01-test-infra    **Depends on:** 01-08, 01-09    **Milestone:** M0.5

## Goal
Ship a `cc-conformance-run` binary (under `crates/rcc_conformance/`)
that runs every configured suite and serialises the `Report` to
`docs/conformance.json`. This file is the machine-readable source of
truth for `docs/conformance.md`.

## Scope
- In: new `bin/cc_conformance_run.rs`; CLI flags: `--rcc <path>`,
  `--suite <name>` (may repeat), `--output <path>`, `--include-gpl`.
- Out: the markdown renderer (task 11).

## Deliverables
- `crates/rcc_conformance/src/bin/cc_conformance_run.rs`.
- `Report::to_json_pretty` is exercised by a new integration test
  asserting round-trip `serde` works.

## Acceptance
- `cargo run --release --package rcc_conformance --bin cc_conformance_run \
    --rcc target/release/rcc --suite c-testsuite --suite chibicc` produces a
  valid JSON with per-case outcomes.
- JSON shape matches the `SuiteReport` / `Outcome` enum in
  `rcc_conformance::*` exactly (documented in `docs/conformance.md`
  as an API contract).

## References
- `rcc_conformance::Report` definition.
