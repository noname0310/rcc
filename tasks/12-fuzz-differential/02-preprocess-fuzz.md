> ✓ done — 2026-05-05 — virtual include-tree fuzzing plus 30m path-filtered workflow

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
- `.github/workflows/fuzz-preprocess-30m.yml`.

## Acceptance
- 30 minute path-filtered or manually dispatched run: 0 new crashes;
  exec/s reasonable (> 100).

## Completion notes
- `Session` now has a session-local virtual file layer used by fuzz
  targets and tests; production driver paths still resolve through the
  host filesystem.
- `Preprocessor::process_include` resolves virtual files through the
  same C99 include-search order as disk files.
- `fuzz/fuzz_targets/preprocess.rs` splits one input into a root TU plus
  fixed virtual headers using `/*__RCC_FUZZ_VIRTUAL_FILE__*/`, avoiding
  per-input temporary files.
- `scripts/fuzz/seed-preprocess.{sh,ps1}` now seed chibicc include
  headers and a bundled multi-file corpus input.
- The extended GitHub workflow is path-filtered and manual-dispatchable,
  with default `-max_total_time=1800`.

## References
- Plan §8.5.
