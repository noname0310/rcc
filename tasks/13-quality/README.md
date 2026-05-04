# 13-quality

**Goal of the phase.** Turn the current supported compiler surface into a
release candidate. This phase is not a place to hide failures with better
percentages: conformance failures, fuzz crashes, coverage holes, and CI drift
must become explicit release blockers or documented non-goals.

## Tasks

| # | File | Summary |
|---|------|---------|
| 01 | [`01-opt-level-wiring.md`](01-opt-level-wiring.md) | `-O0/1/2/3` → LLVM PMB levels. |
| 02 | [`02-diagnostic-quality-sweep.md`](02-diagnostic-quality-sweep.md) | Audit every diagnostic for labels + notes. |
| 03 | [`03-warning-categories.md`](03-warning-categories.md) | Common warnings and `-Wall`/`-Wextra` groups. |
| 04 | [`04-diagnostic-pragmas.md`](04-diagnostic-pragmas.md) | `#pragma GCC diagnostic` push/pop/ignore. |
| 05 | [`05-restrict-noalias.md`](05-restrict-noalias.md) | `restrict` → LLVM `noalias`. |
| 06 | [`06-coverage-threshold.md`](06-coverage-threshold.md) | Make llvm-cov thresholds real and explain misses. |
| 07 | [`07-conformance-release-freeze.md`](07-conformance-release-freeze.md) | Freeze dashboard JSON and xfail policy for release. |
| 08 | [`08-fuzz-regression-artifacts.md`](08-fuzz-regression-artifacts.md) | Promote fuzz crashes into permanent regression seeds. |
| 09 | [`09-ci-green-matrix.md`](09-ci-green-matrix.md) | Require every mandatory GitHub Actions job green. |
| 10 | [`10-bench-harness.md`](10-bench-harness.md) | Build-speed + runtime benchmarks. |
| 11 | [`11-toolchain-platform-matrix.md`](11-toolchain-platform-matrix.md) | Pin supported host/target/toolchain matrix. |
| 12 | [`12-docs-consistency.md`](12-docs-consistency.md) | Sync README/docs/tasks with current behavior. |
| 13 | [`13-release-candidate-dry-run.md`](13-release-candidate-dry-run.md) | Local RC script that runs all release gates. |
| 14 | [`14-release-process.md`](14-release-process.md) | Manual `major|minor|patch` release workflow, GitHub binaries, and crates.io publish as `rcc-compiler`. |

## Exit criteria

- Mandatory CI is green on `main`.
- Conformance dashboard has zero non-xfailed failures in the required suites.
- Fuzz workflows either pass or every crash artifact is reduced into a seed
  with a matching regression test.
- Coverage thresholds are enforced and documented per crate.
- `-O2` and release-mode codegen are measured against a small fixed benchmark
  corpus; regressions become explicit follow-up tasks.
- A signed, tagged release exists on GitHub only after tasks 01-13 are done.
- `cargo install rcc-compiler` installs an executable named `rcc`.
