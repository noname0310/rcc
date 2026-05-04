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
- [x] [15h-gcc-torture-record-typedef-field-lowering](15h-gcc-torture-record-typedef-field-lowering.md)
- [x] [15c-gcc-torture-pointer-comparison-codegen](15c-gcc-torture-pointer-comparison-codegen.md)
- [x] [15d-gcc-torture-vla-layout-codegen](15d-gcc-torture-vla-layout-codegen.md)
- [x] [15e-gcc-torture-runtime-signal-triage](15e-gcc-torture-runtime-signal-triage.md)
- [x] [15i-gcc-torture-aligned-attribute-layout](15i-gcc-torture-aligned-attribute-layout.md)
- [x] [15j-gcc-torture-aggregate-byval-runtime](15j-gcc-torture-aggregate-byval-runtime.md)
- [x] [15k-gcc-torture-runtime-signal-cluster-sweep](15k-gcc-torture-runtime-signal-cluster-sweep.md)
- [x] [15l-gcc-torture-bitfield-precision-cluster](15l-gcc-torture-bitfield-precision-cluster.md)
- [x] [15l1-gcc-torture-wide-bitfield-precision](15l1-gcc-torture-wide-bitfield-precision.md)
- [x] [15m-gcc-torture-scalar-conversion-cluster](15m-gcc-torture-scalar-conversion-cluster.md)
- [x] [15m1-gcc-torture-990222-assignment-result-control](15m1-gcc-torture-990222-assignment-result-control.md)
- [x] [15m2-gcc-torture-20030916-uchar-index-wrap](15m2-gcc-torture-20030916-uchar-index-wrap.md)
- [x] [15n-gcc-torture-vla-lifetime-cluster](15n-gcc-torture-vla-lifetime-cluster.md)
- [x] [15n1-gcc-torture-pr77767-vla-parameter-bound-side-effects](15n1-gcc-torture-pr77767-vla-parameter-bound-side-effects.md)
- [x] [15o-gcc-torture-block-scope-extern](15o-gcc-torture-block-scope-extern.md)
- [x] [15p-gcc-torture-varargs-cluster](15p-gcc-torture-varargs-cluster.md)
- [x] [15q-gcc-torture-aggregate-memory-cluster](15q-gcc-torture-aggregate-memory-cluster.md)
- [x] [15r-gcc-torture-gnu-field-alignment](15r-gcc-torture-gnu-field-alignment.md)
- [x] [15s-gcc-torture-gnu-vector-cluster](15s-gcc-torture-gnu-vector-cluster.md)
- [x] [15s1-gnu-vector-type-layout](15s1-gnu-vector-type-layout.md)
- [x] [15s2-gnu-vector-initializers](15s2-gnu-vector-initializers.md)
- [x] [15s3-gnu-vector-memory](15s3-gnu-vector-memory.md)
- [x] [15s4-gnu-vector-casts](15s4-gnu-vector-casts.md)
- [x] [15s5-gnu-vector-arithmetic](15s5-gnu-vector-arithmetic.md)
- [x] [15s6-gnu-vector-abi](15s6-gnu-vector-abi.md)
- [x] [15s7-gnu-vector-cluster-gate](15s7-gnu-vector-cluster-gate.md)
- [x] [15t-gcc-torture-gnu-builtin-libcalls](15t-gcc-torture-gnu-builtin-libcalls.md)
- [x] [15u-gcc-torture-gnu-inline-asm](15u-gcc-torture-gnu-inline-asm.md)
- [x] [15v-gcc-torture-gnu89-legacy](15v-gcc-torture-gnu89-legacy.md)
- [x] [15w-gcc-torture-scalar-storage-order](15w-gcc-torture-scalar-storage-order.md)
- [x] [16-tcc-tests2](16-tcc-tests2.md)
- [x] [16a-tcc-tests2-float-codegen](16a-tcc-tests2-float-codegen.md)
- [x] [16b-tcc-tests2-multidimensional-array-index](16b-tcc-tests2-multidimensional-array-index.md)
- [x] [16c-tcc-tests2-typedef-function-declarator](16c-tcc-tests2-typedef-function-declarator.md)
- [x] [16d-tcc-tests2-macro-empty-args](16d-tcc-tests2-macro-empty-args.md)
- [x] [16e-tcc-tests2-flexarray-init](16e-tcc-tests2-flexarray-init.md)
- [x] [16f-tcc-tests2-dead-code-cfg-panic](16f-tcc-tests2-dead-code-cfg-panic.md)
- [x] [16g-tcc-tests2-integer-promotion-bitfield](16g-tcc-tests2-integer-promotion-bitfield.md)
- [x] [16h-tcc-tests2-bitfields-layout](16h-tcc-tests2-bitfields-layout.md)
- [x] [16i-tcc-tests2-standard-header-surface](16i-tcc-tests2-standard-header-surface.md)
- [x] [16j-tcc-tests2-binary-floating-literals](16j-tcc-tests2-binary-floating-literals.md)
- [x] [17-llvm-test-suite](17-llvm-test-suite.md)
- [x] [18-xfail-shrink](18-xfail-shrink.md)
- [x] [19-refresh-conformance-dashboard](19-refresh-conformance-dashboard.md)

## Downstream

- 13-quality
