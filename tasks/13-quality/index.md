# 13-quality: index

Release-readiness gate for the supported `rcc` surface: optimization
plumbing, warnings, diagnostics, conformance/fuzz/coverage gates,
toolchain checks, benchmarks, docs, and finally a tagged release.

## Upstream deps

- 09-codegen-llvm, 10-driver, 11-conformance, 12-fuzz-differential

## Tasks (pick in order)

- [x] [01-opt-level-wiring](01-opt-level-wiring.md)
- [x] [02-diagnostic-quality-sweep](02-diagnostic-quality-sweep.md)
- [ ] [03-warning-categories](03-warning-categories.md)
- [ ] [04-diagnostic-pragmas](04-diagnostic-pragmas.md)
- [ ] [05-restrict-noalias](05-restrict-noalias.md)
- [ ] [06-coverage-threshold](06-coverage-threshold.md)
- [ ] [07-conformance-release-freeze](07-conformance-release-freeze.md)
- [ ] [08-fuzz-regression-artifacts](08-fuzz-regression-artifacts.md)
- [ ] [09-ci-green-matrix](09-ci-green-matrix.md)
- [ ] [10-bench-harness](10-bench-harness.md)
- [ ] [11-toolchain-platform-matrix](11-toolchain-platform-matrix.md)
- [ ] [12-docs-consistency](12-docs-consistency.md)
- [ ] [13-release-candidate-dry-run](13-release-candidate-dry-run.md)
- [ ] [14-release-process](14-release-process.md)

## Downstream

- (release)
