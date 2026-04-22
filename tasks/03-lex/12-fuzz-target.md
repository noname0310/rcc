# 03-12: Fuzz target (24 h no-panic)

**Phase:** 03-lex    **Depends on:** 03-11    **Milestone:** M1

## Goal
Promote the existing stub `fuzz/fuzz_targets/lex.rs` to a real target
with a seed corpus pulled from `c-testsuite`. Nightly CI runs it for
an hour; local 24 h runs are expected before each milestone tag.

## Scope
- In: `fuzz/corpus/lex/` seeded (copy-on-write) from c-testsuite;
  `.cargo/config.toml` for the fuzz workspace pointing at LibFuzzer
  options (`-max_len=131072`, ASAN enabled).
- Out: differential fuzzing (phase 12).

## Deliverables
- Seed script `scripts/fuzz/seed-lex.sh` populating the corpus.
- `README.md` in `fuzz/` describing `cargo +nightly fuzz run lex`.

## Acceptance
- 30-second CI smoke run finishes with 0 crashes.
- Local 10-minute run seeded from the corpus produces no new crashes.
- On introduction of a deliberate panic (e.g. `panic!()` in cursor),
  the fuzzer catches it within seconds.

## References
- Plan §8.5.
