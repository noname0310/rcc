# 13-quality: index

Release-readiness gate for the supported `rcc` surface: optimization
plumbing, warnings, diagnostics, conformance/fuzz/coverage gates,
toolchain checks, benchmarks, docs, and finally a tagged release.

## Upstream deps

- 09-codegen-llvm, 10-driver, 11-conformance, 12-fuzz-differential

## Tasks (pick in order)

- [x] [01-opt-level-wiring](01-opt-level-wiring.md)
- [x] [02-diagnostic-quality-sweep](02-diagnostic-quality-sweep.md)
- [x] [03-warning-categories](03-warning-categories.md)
- [x] [03a-unused-variable-warning](03a-unused-variable-warning.md)
- [x] [03b-unused-function-warning](03b-unused-function-warning.md)
- [x] [03c-unused-parameter-warning](03c-unused-parameter-warning.md)
- [x] [03d-implicit-function-declaration-warning](03d-implicit-function-declaration-warning.md)
- [x] [03e-sign-compare-warning](03e-sign-compare-warning.md)
- [x] [03f-unreachable-code-warning](03f-unreachable-code-warning.md)
- [x] [04-diagnostic-pragmas](04-diagnostic-pragmas.md)
- [x] [05-restrict-noalias](05-restrict-noalias.md)
- [x] [06-coverage-threshold](06-coverage-threshold.md)
- [x] [07-conformance-release-freeze](07-conformance-release-freeze.md)
- [x] [08-fuzz-regression-artifacts](08-fuzz-regression-artifacts.md)
- [x] [09-ci-green-matrix](09-ci-green-matrix.md)
- [x] [10-bench-harness](10-bench-harness.md)
- [x] [11-toolchain-platform-matrix](11-toolchain-platform-matrix.md)
- [x] [12-docs-consistency](12-docs-consistency.md)
- [x] [13-release-candidate-dry-run](13-release-candidate-dry-run.md)
- [ ] [14-release-process](14-release-process.md)

## Downstream

- (release)
