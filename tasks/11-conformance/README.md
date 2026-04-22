# 11-conformance

**Goal of the phase.** Each task below corresponds to one **KPI cell**
in [`00-overview/02-kpi-dashboard.md`](../00-overview/02-kpi-dashboard.md).
Closing a task means the numbers in `docs/conformance.md` rise past
the target and stay there.

## Tasks

| # | File | Cell (suite × milestone) |
|---|------|---------|
| 01 | [`01-ctestsuite-m3-40pct.md`](01-ctestsuite-m3-40pct.md) | c-testsuite @ M3 ≥ 40 %. |
| 02 | [`02-ctestsuite-m4-70pct.md`](02-ctestsuite-m4-70pct.md) | c-testsuite @ M4 ≥ 70 %. |
| 03 | [`03-ctestsuite-m6-95pct.md`](03-ctestsuite-m6-95pct.md) | c-testsuite @ M6 ≥ 95 %. |
| 04 | [`04-chibicc-stages-1-3.md`](04-chibicc-stages-1-3.md) | chibicc stages 1..3 @ M2. |
| 05 | [`05-chibicc-preprocess.md`](05-chibicc-preprocess.md) | chibicc preprocessor @ M5. |
| 06 | [`06-gcc-torture-smoke.md`](06-gcc-torture-smoke.md) | gcc-torture smoke @ M4. |
| 07 | [`07-gcc-torture-60pct.md`](07-gcc-torture-60pct.md) | gcc-torture @ M6 ≥ 60 %. |
| 08 | [`08-tcc-tests2.md`](08-tcc-tests2.md) | tcc-tests2 @ M6. |
| 09 | [`09-llvm-test-suite.md`](09-llvm-test-suite.md) | llvm-test-suite @ M7. |
| 10 | [`10-xfail-shrink.md`](10-xfail-shrink.md) | Continuously shrink xfail lists. |

## Exit criteria per task

Each task is "done" when two things are both true:
1. `docs/conformance.md`'s relevant cell meets the target number on
   CI's next run.
2. The cell **stays** green on three consecutive CI runs.
