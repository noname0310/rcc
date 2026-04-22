# 01-test-infra

**Goal of the phase.** Before a single line of lexer logic is written,
we guarantee that (a) every external suite downloads reproducibly and
(b) the conformance runner can *already report 0 %* against each suite.
Once this phase is green, every later task can be judged by moving its
cells in [`docs/conformance.md`](../../docs/conformance.md).

## Tasks

| # | File | Summary |
|---|------|---------|
| 01 | [`01-fetch-ctestsuite.md`](01-fetch-ctestsuite.md) | Vendor `c-testsuite`, pin commit. |
| 02 | [`02-fetch-chibicc.md`](02-fetch-chibicc.md) | Sparse-checkout `chibicc/test`. |
| 03 | [`03-fetch-gcc-torture.md`](03-fetch-gcc-torture.md) | GPL gate for gcc-torture. |
| 04 | [`04-fetch-tcc-tests2.md`](04-fetch-tcc-tests2.md) | Sparse tcc `tests2`. |
| 05 | [`05-fetch-llvm-test-suite.md`](05-fetch-llvm-test-suite.md) | llvm-test-suite SingleSource. |
| 06 | [`06-fetch-csmith.md`](06-fetch-csmith.md) | Pin + build csmith. |
| 07 | [`07-pin-manifest-revs.md`](07-pin-manifest-revs.md) | Replace all branch refs with commit SHAs. |
| 08 | [`08-ctestsuite-adapter.md`](08-ctestsuite-adapter.md) | Implement `CTestSuiteAdapter`. |
| 09 | [`09-chibicc-adapter.md`](09-chibicc-adapter.md) | Implement `ChibiccAdapter`. |
| 10 | [`10-conformance-report-json.md`](10-conformance-report-json.md) | Write `docs/conformance.json`. |
| 11 | [`11-conformance-dashboard-md.md`](11-conformance-dashboard-md.md) | Render markdown table. |
| 12 | [`12-xfail-seed.md`](12-xfail-seed.md) | Seed empty `xfail.toml` per suite. |
| 13 | [`13-ci-wire-conformance.md`](13-ci-wire-conformance.md) | Block CI on the milestone subset. |

## Exit criteria

1. `cargo xtask fetch-testsuites` completes cleanly on a fresh
   checkout (permissive suites only; `--include-gpl` adds the rest).
2. `cargo run --release --package rcc_conformance` produces a report
   where every suite column shows `Discovered > 0, Pass = 0`.
3. CI has a `conformance` job that fails if any configured cell
   misses its target.
