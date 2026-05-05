# Coverage

Coverage is enforced by `cargo xtask coverage`, which wraps `cargo llvm-cov`.
The gate is intentionally honest rather than aspirational: it enforces the
current release baseline and names low-coverage crates instead of claiming that
every crate is already at 80%.

## Command

```bash
cargo xtask coverage --lcov lcov.info --json target/coverage/coverage-summary.json
```

The wrapper:

- runs `cargo llvm-cov --workspace --json --summary-only`;
- writes `lcov.info`, `target/coverage/coverage-summary.json`, and
  `target/coverage/coverage-report.txt`;
- excludes vendored suites, fuzz corpora/artifacts, `target/`, and generated
  snapshot files from the denominator;
- fails if the workspace or any crate drops below its documented threshold.

## Thresholds

Baseline measured on 2026-05-05 with Rust 1.95.0 and cargo-llvm-cov 0.8.5.

| Crate | Current line coverage | Threshold | Note |
| ----- | --------------------- | --------- | ---- |
| `workspace` | 84.37% | 80% | release gate |
| `rcc_ast` | 6.40% | 5% | visitor traversal is mostly exercised indirectly later |
| `rcc_cfg` | 87.12% | 80% | core CFG builder/lower/verifier |
| `rcc_cfg_transform` | 0.00% | 0% | placeholder pass trait |
| `rcc_codegen_llvm` | 88.59% | 80% | no-LLVM unit coverage; LLVM FileCheck runs separately |
| `rcc_conformance` | 50.21% | 45% | subprocess adapters have CI-only paths |
| `rcc_data_structures` | 74.47% | 70% | small macro-generated index helpers |
| `rcc_driver` | 62.44% | 60% | integration-heavy and platform-gated paths |
| `rcc_errors` | 93.79% | 85% | diagnostic policy surface |
| `rcc_hir` | 77.24% | 75% | layout service covered; pretty helpers are lighter |
| `rcc_hir_lower` | 83.54% | 80% | release-critical lowering |
| `rcc_lexer` | 96.37% | 90% | dense table/corpus/fuzz coverage |
| `rcc_parse` | 87.03% | 80% | grammar coverage |
| `rcc_preprocess` | 93.87% | 90% | macro/include/conditional coverage |
| `rcc_session` | 100.00% | 90% | small session option surface |
| `rcc_span` | 91.67% | 85% | source map and symbol APIs |
| `rcc_typeck` | 87.27% | 80% | semantic checks |
| `xtask` | 66.21% | 60% | subprocess automation paths |

## LLVM-only Tests

The default coverage job is a no-LLVM run. That means `#[cfg(feature = "llvm")]`
FileCheck tests under `rcc_codegen_llvm` are not counted in the JSON summary.
They are covered by the separate CI job:

```bash
cargo test --workspace --features rcc_codegen_llvm/llvm
```

This split is deliberate: coverage tracks the default portable workspace, while
the LLVM job proves backend-specific behavior with LLVM 18 installed.
