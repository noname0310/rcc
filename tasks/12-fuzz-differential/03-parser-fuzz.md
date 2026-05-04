> ✓ done — 2026-05-05 — parse fuzz target, seed corpus, and 30m workflow

# 12-03: Parser fuzz

**Phase:** 12-fuzz-differential    **Depends on:** 05-30    **Milestone:** M2+

## Goal
Add a parse-only fuzz target: bytes → lex → preprocess → parse.
Useful once the parser is feature-complete.

## Scope
- In: `fuzz/fuzz_targets/parse.rs`; seed from c-testsuite + chibicc.
- Out: differential comparisons (task 04).

## Deliverables
- Target + seed script.

## Acceptance
- No panics in a 30 minute path-filtered or manually dispatched run.
- Parsed token count per byte within a sane range (diagnostics
  guard against pathological blow-ups).

## Completion notes
- Added `fuzz/fuzz_targets/parse.rs`, running bytes through
  `preprocess` and `rcc_parse::parse` with captured diagnostics.
- Added guardrails for parser-focused fuzzing: 128 KiB max input,
  64 Ki preprocessed-token cap, 64x token/input ratio cap, and a
  2048 diagnostic cap before parsing.
- Added `scripts/fuzz/seed-parse.{sh,ps1}` and 22 curated parse seeds
  from c-testsuite + chibicc.
- Added `.github/workflows/fuzz-parse-30m.yml`, path-filtered and
  manually dispatchable with default `-max_total_time=1800`.

## References
- Plan §8.5.
