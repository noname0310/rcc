# 12-02: Preprocessor fuzz

**Phase:** 12-fuzz-differential    **Depends on:** 04-19    **Milestone:** M5

## Goal
Extend the preprocessor fuzz target to cover `#include` behaviour by
giving libfuzzer access to a small virtual filesystem.

## Scope
- In: wrap `SourceMap::load_file` so the fuzzer can synthesise
  headers inline from the input bytes; seed corpus from chibicc.
- Out: huge macro-heavy corpora (future: mine chromium preprocessed
  headers).

## Deliverables
- `fuzz/fuzz_targets/preprocess.rs` upgrades.
- Seed script.

## Acceptance
- 24 h nightly: 0 new crashes; exec/s reasonable (> 100).

## References
- Plan §8.5.
