# 11-conformance

**Goal of the phase.** Each task below either corresponds to one **KPI cell**
in [`00-overview/02-kpi-dashboard.md`](../00-overview/02-kpi-dashboard.md)
or turns an external-suite failure into an owned compiler-bug task.
Closing a KPI task means the numbers in `docs/conformance.md` rise past
the target and stay there.

Pass rate is not the source of truth for correctness. If a failing TU exposes
a C99 compiler bug, fix or task the bug even when the current numeric target
already passes. XFAIL is only for explicit non-C99 extensions, unsupported
future milestones, or documented policy decisions.

## Tasks

| # | File | Cell (suite × milestone) |
|---|------|---------|
| 01 | [`01-ctestsuite-m3-40pct.md`](01-ctestsuite-m3-40pct.md) | c-testsuite @ M3 ≥ 40 %. |
| 02 | [`02-ctestsuite-m4-70pct.md`](02-ctestsuite-m4-70pct.md) | c-testsuite @ M4 ≥ 70 %. |
| 03 | [`03-ctestsuite-m6-95pct.md`](03-ctestsuite-m6-95pct.md) | c-testsuite @ M6 ≥ 95 %. |
| 04 | [`04-ctestsuite-residual-bug-triage.md`](04-ctestsuite-residual-bug-triage.md) | classify residual c-testsuite failures. |
| 05 | [`05-chibicc-stage-isolation.md`](05-chibicc-stage-isolation.md) | stage-isolated chibicc mode. |
| 06 | [`06-gnu-binary-integer-literals.md`](06-gnu-binary-integer-literals.md) | GNU `0b...` literals. |
| 07 | [`07-gnu-statement-expression-semantics.md`](07-gnu-statement-expression-semantics.md) | GNU `({ ... })` semantics. |
| 08 | [`08-chibicc-arith-green.md`](08-chibicc-arith-green.md) | chibicc `arith.c` green. |
| 09 | [`09-gnu-control-flow-extensions.md`](09-gnu-control-flow-extensions.md) | GNU case ranges + computed goto. |
| 10 | [`10-chibicc-control-green.md`](10-chibicc-control-green.md) | chibicc `control.c` green. |
| 11 | [`11-chibicc-function-prereq-triage.md`](11-chibicc-function-prereq-triage.md) | classify `function.c` blockers. |
| 12 | [`12-chibicc-function-green.md`](12-chibicc-function-green.md) | chibicc `function.c` green. |
| 13 | [`13-chibicc-preprocess.md`](13-chibicc-preprocess.md) | chibicc preprocessor @ M5. |
| 14 | [`14-gcc-torture-smoke.md`](14-gcc-torture-smoke.md) | gcc-torture smoke @ M4. |
| 15 | [`15-gcc-torture-60pct.md`](15-gcc-torture-60pct.md) | gcc-torture @ M6 ≥ 60 %. |
| 16 | [`16-tcc-tests2.md`](16-tcc-tests2.md) | tcc-tests2 @ M6. |
| 17 | [`17-llvm-test-suite.md`](17-llvm-test-suite.md) | llvm-test-suite @ M7. |
| 18 | [`18-xfail-shrink.md`](18-xfail-shrink.md) | Continuously shrink xfail lists. |

## Exit criteria per task

KPI tasks are "done" when two things are both true:
1. `docs/conformance.md`'s relevant cell meets the target number on
   CI's next run.
2. The cell **stays** green on three consecutive CI runs.

Triage tasks are "done" only when every failing TU is classified and every
C99 compiler bug has an owner task. A compiler panic is always a compiler bug.
