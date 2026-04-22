# 13-03: Benchmark harness

**Phase:** 13-quality    **Depends on:** 13-01    **Milestone:** M7

## Goal
Two dimensions of perf:
1. **Compile speed** — `criterion`-based micro-bench on
   `rcc_lexer::tokenize`, `rcc_parse::parse`, and full pipeline for
   `hello.c`, `sqlite-amalgamation`, `ffmpeg libavcodec` (subset).
2. **Runtime of generated code** — lightweight SPEC-like subset;
   diff vs host `cc -O2`.

## Scope
- In: `benches/` under relevant crates; a `scripts/bench-runtime.sh`
  that runs small programs and reports wall-clock.
- Out: proper SPEC (license).

## Deliverables
- `benches/lex.rs`, `benches/parse.rs`, `benches/pipeline.rs`.
- Runtime-comparison script + report.

## Acceptance
- `cargo bench` runs without flake.
- Report shows `rcc -O2` runtime within 2× of host `cc -O2`.

## References
- `criterion` docs.
