# 13-quality

**Goal of the phase.** Post-M6 polish: optimisation levels, diagnostic
review pass, performance benchmarking, and release process.

## Tasks

| # | File | Summary |
|---|------|---------|
| 01 | [`01-opt-level-wiring.md`](01-opt-level-wiring.md) | `-O0/1/2/3` → LLVM PMB levels. |
| 02 | [`02-diagnostic-quality-sweep.md`](02-diagnostic-quality-sweep.md) | Audit every diagnostic for labels + notes. |
| 03 | [`03-bench-harness.md`](03-bench-harness.md) | Build-speed + runtime benchmarks. |
| 04 | [`04-release-process.md`](04-release-process.md) | Cut `v0.1.0`. |
| 05 | [`05-diagnostic-pragmas.md`](05-diagnostic-pragmas.md) | `#pragma GCC diagnostic` push/pop/ignore. |
| 06 | [`06-restrict-noalias.md`](06-restrict-noalias.md) | `restrict` → LLVM `noalias`. |
| 07 | [`07-warning-categories.md`](07-warning-categories.md) | Common warnings and `-Wall`/`-Wextra` groups. |

## Exit criteria

- `-O2` produces a binary that is ≤ 2× host `cc -O2` runtime on a
  SPEC-lite subset.
- Every diagnostic has been eyeballed once by a human.
- A signed, tagged release exists on GitHub.
