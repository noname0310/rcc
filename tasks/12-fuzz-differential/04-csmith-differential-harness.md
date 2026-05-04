> ✓ done — 2026-05-05 — added `rcc_csmith_diff` harness binary

# 12-04: csmith differential harness

**Phase:** 12-fuzz-differential    **Depends on:** 01-06, 10-06    **Milestone:** M7

## Goal
Generate random C programs with csmith, compile + run them with both
`rcc` and host `cc`, compare exit codes and stdout. Disagreement =
miscompile candidate.

## Scope
- In: `scripts/csmith-diff.sh` or bin under `rcc_conformance`; seed
  csmith with the current timestamp; bounded program size (< 10 KB
  to keep compile time < 5 s).
- Out: auto-bisection of root-cause (future).

## Deliverables
- Harness binary.
- Instructions under
  `third_party/testsuites/csmith/INSTALL.md`.

## Acceptance
- Running the harness for an hour on a healthy compiler yields 0
  disagreements.

## Completion notes
- Added `cargo run --release --package rcc_conformance --bin rcc_csmith_diff`
  as the harness binary.
- The harness generates bounded csmith programs, compiles/runs them with
  host `cc` and `rcc`, normalizes stdout newlines, and reports JSON under
  `target/csmith-diff/report.json` by default.
- Source generation is bounded by `--max-source-bytes` and per-command
  `--timeout-secs`; large generated programs are skipped rather than used
  as release-gate noise.
- `xtask` now writes `third_party/testsuites/csmith/INSTALL.md` with
  concrete build and harness invocation commands.

## References
- Yang, Chen, Eide, Regehr. "Finding and understanding bugs in C
  compilers." (2011).
