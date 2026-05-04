# 12-fuzz-differential

**Goal of the phase.** Long-running oracle-style verification:
cargo-fuzz targets for every front-end crate + csmith-driven
differential fuzzing against host `cc`. Runs parallel to phases
03..09, gaining targets as crates come online.

## Tasks

| # | File | Summary |
|---|------|---------|
| 01 | [`01-lexer-fuzz-30m.md`](01-lexer-fuzz-30m.md) | 30 minute path-filtered lexer run. |
| 02 | [`02-preprocess-fuzz.md`](02-preprocess-fuzz.md) | preprocessor fuzz. |
| 03 | [`03-parser-fuzz.md`](03-parser-fuzz.md) | parser fuzz. |
| 04 | [`04-csmith-differential-harness.md`](04-csmith-differential-harness.md) | csmith vs cc runner. |
| 05 | [`05-csmith-bounded-differential.md`](05-csmith-bounded-differential.md) | bounded csmith budget. |

## Exit criteria

- Each target has a seed corpus under `fuzz/corpus/<target>/`.
- Nightly CI produces a single artefact `reports/fuzz-YYYYMMDD.json`
  with crash counts + exec/s.
