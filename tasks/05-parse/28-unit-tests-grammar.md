# 05-28: Grammar-production unit tests

**Phase:** 05-parse    **Depends on:** 05-07 .. 05-26    **Milestone:** M2

## Goal
One `#[test]` per grammar production (roughly): positive case
(accepts), negative case (rejects with specific diagnostic). Grouped
into files by chapter of C99 §6.

## Scope
- In: `crates/rcc_parse/tests/grammar/*.rs` following the §6.5 /
  §6.7 / §6.8 / §6.9 split; each test uses the `run_table` helper.
- Out: corpus-wide smoke (task 30).

## Deliverables
- ~50-80 `#[test]` functions.
- Every C99 §6 production has at least one test.

## Acceptance
- `cargo test -p rcc_parse --test grammar`: green.
- `cargo llvm-cov`: `rcc_parse` ≥ 80 % line coverage.

## References
- C99 §6.
