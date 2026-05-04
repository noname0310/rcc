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
| 11a | [`11a-function-stage-common-support.md`](11a-function-stage-common-support.md) | host-compiled chibicc `common` support for `function.c`. |
| 11b | [`11b-function-name-predefined-identifiers.md`](11b-function-name-predefined-identifiers.md) | C99 `__func__` and GNU `__FUNCTION__`. |
| 11c | [`11c-function-va-area-compat.md`](11c-function-va-area-compat.md) | chibicc `__va_area__` compatibility. |
| 11d | [`11d-function-abi-runtime-smoke.md`](11d-function-abi-runtime-smoke.md) | reduced ABI/runtime slices for `function.c`. |
| 12 | [`12-chibicc-function-green.md`](12-chibicc-function-green.md) | chibicc `function.c` green. |
| 13 | [`13-chibicc-preprocess.md`](13-chibicc-preprocess.md) | chibicc preprocessor @ M5. |
| 14 | [`14-gcc-torture-smoke.md`](14-gcc-torture-smoke.md) | gcc-torture smoke @ M4. |
| 15 | [`15-gcc-torture-60pct.md`](15-gcc-torture-60pct.md) | gcc-torture @ M6 ≥ 60 %. |
| 16 | [`16-tcc-tests2.md`](16-tcc-tests2.md) | tcc-tests2 @ M6. |
| 16a | [`16a-tcc-tests2-float-codegen.md`](16a-tcc-tests2-float-codegen.md) | tcc-tests2 float codegen failures. |
| 16b | [`16b-tcc-tests2-multidimensional-array-index.md`](16b-tcc-tests2-multidimensional-array-index.md) | tcc-tests2 multidimensional array indexing. |
| 16c | [`16c-tcc-tests2-typedef-function-declarator.md`](16c-tcc-tests2-typedef-function-declarator.md) | tcc-tests2 typedef/function declarator parse bug. |
| 16d | [`16d-tcc-tests2-macro-empty-args.md`](16d-tcc-tests2-macro-empty-args.md) | tcc-tests2 empty macro arguments. |
| 16e | [`16e-tcc-tests2-flexarray-init.md`](16e-tcc-tests2-flexarray-init.md) | tcc-tests2 flexible-array initializer/typeck gap. |
| 16f | [`16f-tcc-tests2-dead-code-cfg-panic.md`](16f-tcc-tests2-dead-code-cfg-panic.md) | tcc-tests2 dead-code CFG panic. |
| 16g | [`16g-tcc-tests2-integer-promotion-bitfield.md`](16g-tcc-tests2-integer-promotion-bitfield.md) | tcc-tests2 integer promotion on narrow/bit-field values. |
| 16h | [`16h-tcc-tests2-bitfields-layout.md`](16h-tcc-tests2-bitfields-layout.md) | tcc-tests2 bit-field layout mismatches. |
| 16i | [`16i-tcc-tests2-standard-header-surface.md`](16i-tcc-tests2-standard-header-surface.md) | tcc-tests2 host standard-header surface. |
| 16j | [`16j-tcc-tests2-binary-floating-literals.md`](16j-tcc-tests2-binary-floating-literals.md) | TinyCC binary floating literal compatibility policy. |
| 17 | [`17-llvm-test-suite.md`](17-llvm-test-suite.md) | llvm-test-suite @ M7. |
| 18 | [`18-xfail-shrink.md`](18-xfail-shrink.md) | Continuously shrink xfail lists. |
| 19 | [`19-refresh-conformance-dashboard.md`](19-refresh-conformance-dashboard.md) | Refresh stale dashboard fail rows. |

## Exit criteria per task

KPI tasks are "done" when two things are both true:
1. `docs/conformance.md`'s relevant cell meets the target number on
   CI's next run.
2. The cell **stays** green on three consecutive CI runs.

Triage tasks are "done" only when every failing TU is classified and every
C99 compiler bug has an owner task. A compiler panic is always a compiler bug.
