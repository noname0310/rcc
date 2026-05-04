# 11-conformance: index

Each task either owns one KPI cell in 00-overview/02-kpi-dashboard.md or
turns an external-suite failure into a concrete compiler-bug task. Pass rate
is a gate, not a hiding place: C99 compiler bugs must be fixed even when the
current numeric milestone already passes.

## Upstream deps

- 01-test-infra, 10-driver

## Tasks (pick in order)

- [x] [01-ctestsuite-m3-40pct](01-ctestsuite-m3-40pct.md)
- [x] [02-ctestsuite-m4-70pct](02-ctestsuite-m4-70pct.md)
- [x] [03-ctestsuite-m6-95pct](03-ctestsuite-m6-95pct.md)
- [x] [04-ctestsuite-residual-bug-triage](04-ctestsuite-residual-bug-triage.md)
- [x] [05-chibicc-stage-isolation](05-chibicc-stage-isolation.md)
- [x] [06-gnu-binary-integer-literals](06-gnu-binary-integer-literals.md)
- [x] [07-gnu-statement-expression-semantics](07-gnu-statement-expression-semantics.md)
- [x] [08-chibicc-arith-green](08-chibicc-arith-green.md)
- [x] [09-gnu-control-flow-extensions](09-gnu-control-flow-extensions.md)
- [x] [10-chibicc-control-green](10-chibicc-control-green.md)
- [x] [11-chibicc-function-prereq-triage](11-chibicc-function-prereq-triage.md)
- [x] [11a-function-stage-common-support](11a-function-stage-common-support.md)
- [x] [11b-function-name-predefined-identifiers](11b-function-name-predefined-identifiers.md)
- [x] [11c-function-va-area-compat](11c-function-va-area-compat.md)
- [x] [11d-function-abi-runtime-smoke](11d-function-abi-runtime-smoke.md)
- [x] [12-chibicc-function-green](12-chibicc-function-green.md)
- [x] [13-chibicc-preprocess](13-chibicc-preprocess.md)
- [x] [14-gcc-torture-smoke](14-gcc-torture-smoke.md)
- [x] [15-gcc-torture-60pct](15-gcc-torture-60pct.md)
- [x] [15a-gcc-torture-parser-declaration-forms](15a-gcc-torture-parser-declaration-forms.md)
- [x] [15f-gcc-torture-deferred-macro-rescan](15f-gcc-torture-deferred-macro-rescan.md)
- [x] [15b-gcc-torture-remaining-builtin-compat](15b-gcc-torture-remaining-builtin-compat.md)
- [x] [15g-gcc-torture-overflow-builtins](15g-gcc-torture-overflow-builtins.md)
- [ ] [15h-gcc-torture-record-typedef-field-lowering](15h-gcc-torture-record-typedef-field-lowering.md)
- [ ] [15c-gcc-torture-pointer-comparison-codegen](15c-gcc-torture-pointer-comparison-codegen.md)
- [ ] [15d-gcc-torture-vla-layout-codegen](15d-gcc-torture-vla-layout-codegen.md)
- [ ] [15e-gcc-torture-runtime-signal-triage](15e-gcc-torture-runtime-signal-triage.md)
- [ ] [16-tcc-tests2](16-tcc-tests2.md)
- [ ] [17-llvm-test-suite](17-llvm-test-suite.md)
- [ ] [18-xfail-shrink](18-xfail-shrink.md)

## Downstream

- 13-quality
