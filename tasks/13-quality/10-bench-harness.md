# 13-10: Benchmark harness

**Phase:** 13-quality    **Depends on:** 13-01, 13-09    **Milestone:** M7

## Goal
Two dimensions of perf:
1. **Compile speed** — `criterion`-based micro-bench on
   `rcc_lexer::tokenize`, preprocessing, parsing, and the full driver
   pipeline for small fixed fixtures.
2. **Runtime of generated code** — lightweight SPEC-like subset;
   diff vs host `cc -O2`.

## Scope
- In: `benches/` under relevant crates; a `scripts/bench-runtime.sh`
  or Rust `xtask bench-runtime` that runs small programs and reports
  wall-clock.
- Out:
  - Proper SPEC (license).
  - Large third-party programs that make CI flaky.

## Deliverables
- `benches/lex.rs`, `benches/parse.rs`, `benches/pipeline.rs`.
- Runtime-comparison script + report.
- `docs/perf-baseline.md` with date, host, command, and numbers.

## Acceptance
- `cargo bench --workspace` or documented per-crate bench command runs without
  relying on vendored GPL suites.
- Runtime report contains at least 5 programs and clearly separates compile
  time from generated-code runtime.
- Numbers are not used as a hard pass/fail unless the threshold is documented
  in this task.

## References
- `criterion` docs.
