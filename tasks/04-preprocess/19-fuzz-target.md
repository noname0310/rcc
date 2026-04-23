> ✓ done — 2026-04-23

# 04-19: Fuzz target

**Phase:** 04-preprocess    **Depends on:** 04-17    **Milestone:** M5

## Goal
Wire `fuzz/fuzz_targets/preprocess.rs` (skeleton exists) to exercise
the full pipeline: `Session::new` → `SourceMap::add_file(data)` →
`preprocess()`. No panic / timeout under libfuzzer.

## Scope
- In: populate `fuzz/corpus/preprocess/` with valid tiny programs
  from chibicc; `cargo +nightly fuzz run preprocess` in CI for 60 s.
- Out: differential fuzzing with cc (phase 12).

## Deliverables
- Seeded corpus.
- CI smoke addition.

## Acceptance
- 60 s CI run, 0 crashes, 0 timeouts.
- Manually-introduced bug (stack overflow on recursive macro) is
  caught within seconds.

## References
- Plan §8.5.
