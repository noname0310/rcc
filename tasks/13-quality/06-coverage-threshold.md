# 13-06: Coverage threshold and llvm-cov hygiene

> ✓ done — 2026-05-05

**Phase:** 13-quality    **Depends on:** 12-05    **Milestone:** M7

## Goal
Make the coverage gate honest. `docs/testing.md` says coverage is enforced,
but the CI command must prove that with thresholds and a clear exemption
policy.

## Scope
- In:
  - Add workspace and per-crate coverage thresholds to the CI command or an
    `xtask coverage` wrapper.
  - Exclude generated fixtures, vendored testsuites, and fuzz corpora from the
    denominator.
  - Produce a short report naming crates below threshold and why.
  - Keep `rcc_codegen_llvm` LLVM-only tests from disappearing in no-LLVM
    coverage runs without explanation.
- Out:
  - Chasing 100% coverage.
  - Counting external conformance source lines as project coverage.

## Deliverables
- CI coverage command with explicit threshold flags.
- `docs/coverage.md` with current crate-level numbers.
- Tests or script checks that fail if coverage artifacts are missing.

## Acceptance
- `cargo llvm-cov --workspace` in CI fails below the documented threshold.
- The threshold matches `docs/testing.md`; no doc says coverage is enforced
  unless CI really enforces it.
- The coverage artifact is uploaded on failure and success.

## References
- `cargo-llvm-cov` threshold flags.
- `docs/testing.md`.
