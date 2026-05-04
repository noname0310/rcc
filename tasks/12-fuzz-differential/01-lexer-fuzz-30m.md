> ✓ done — 2026-05-04
> policy revised — 2026-05-05 — 30m path-filtered personal-project gate

# 12-01: Lexer 30m extended fuzz gate

**Phase:** 12-fuzz-differential    **Depends on:** 03-12    **Milestone:** M1+

## Goal
Run the existing `fuzz/fuzz_targets/lex.rs` target for 30 minutes when
lexer/fuzz-related paths change, or when the workflow is manually
dispatched. Any new crash blocks the next release.

## Scope
- In: path-filtered GitHub Actions workflow; upload corpus + crash
  artefacts; integrate crash triage into local tasks.
- Out: structured grammar-guided fuzzing (future).

## Deliverables
- `.github/workflows/fuzz-lex-30m.yml` with a 30 minute lexer budget.
- GitHub Actions failure notification + uploaded crash artifacts.

## Acceptance
- One path-triggered or manually dispatched 30 minute lex fuzz run with
  0 new unique crashes.

## Completion note
- Configured `.github/workflows/fuzz-lex-30m.yml` as one Linux shard
  with `-max_total_time=1800`.
- Local WSL smoke run passed with `cargo +nightly fuzz run lex --target
  x86_64-unknown-linux-gnu -- -runs=1 -max_len=131072`.
- The 30 minute criterion is an operational release gate: it is
  satisfied by workflow history, not by this local implementation
  commit.

## References
- Plan §8.5.
