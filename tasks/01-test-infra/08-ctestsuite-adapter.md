# 01-08: Implement `CTestSuiteAdapter`

**Phase:** 01-test-infra    **Depends on:** 01-01, 01-07    **Milestone:** M0.5

## Goal
Replace the stub in `crates/rcc_conformance/src/adapters.rs` with a
working adapter. c-testsuite's contract: each test is `NNNNN.c`; the
expected standard output is `NNNNN.c.expected`; run with no arguments,
zero exit code, stdout must match byte-for-byte.

## Scope
- In: `discover()` scans `tests/single-exec/*.c`; `run()` compiles
  with `rcc --emit=obj` → links via host `cc` (or `ld`) → executes →
  compares `stdout` + `exit code`.
- Out: chibicc adapter (task 09), report writing (task 10).

## Deliverables
- `CTestSuiteAdapter::discover` returns every `*.c` in
  `tests/single-exec/`.
- `CTestSuiteAdapter::run` invokes the `rcc` binary located at the
  `rcc_path` argument; timeout of 30 s per test (longer tests are a
  smell).
- Unit tests using 2-3 checked-in fixture files (under
  `crates/rcc_conformance/tests/fixtures/`) exercising `Pass` / `Fail`
  / `Skip` outcomes.

## Acceptance
- `cargo test -p rcc_conformance --test ctestsuite_adapter` green.
- A dry run against the real suite with a hand-rolled echo-exit
  `rcc` binary reports ≥ 200 discovered test cases.

## References
- c-testsuite `README.md` for its test naming + expected-file convention.
- Plan §9.3.
