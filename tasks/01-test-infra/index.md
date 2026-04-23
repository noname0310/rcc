# 01-test-infra: index

Vendor every external C test suite and stand up the conformance
harness. Without this phase no later task can be scored.

## Upstream deps (must be `[x]` globally)

- None (root phase).

## Tasks (pick in order)

- [x] [01-fetch-ctestsuite](01-fetch-ctestsuite.md)
- [x] [02-fetch-chibicc](02-fetch-chibicc.md)
- [x] [03-fetch-gcc-torture](03-fetch-gcc-torture.md)
- [x] [04-fetch-tcc-tests2](04-fetch-tcc-tests2.md)
- [x] [05-fetch-llvm-test-suite](05-fetch-llvm-test-suite.md)
- [x] [06-fetch-csmith](06-fetch-csmith.md)
- [x] [07-pin-manifest-revs](07-pin-manifest-revs.md)
- [x] [08-ctestsuite-adapter](08-ctestsuite-adapter.md)
- [x] [09-chibicc-adapter](09-chibicc-adapter.md)
- [x] [10-conformance-report-json](10-conformance-report-json.md)
- [ ] [11-conformance-dashboard-md](11-conformance-dashboard-md.md)
- [ ] [12-xfail-seed](12-xfail-seed.md)
- [ ] [13-ci-wire-conformance](13-ci-wire-conformance.md)

## Downstream

Unblocks every other implementation phase. Specifically required by
`11-conformance` (numeric gates) and by every "run against suite"
task in phases 03–09.
