# 12-01: Lexer 24 h nightly

**Phase:** 12-fuzz-differential    **Depends on:** 03-12    **Milestone:** M1+

## Goal
Commit to running `cargo fuzz run lex` for 24 h on every release
candidate. Any new crash blocks the release.

## Scope
- In: nightly GitHub Actions workflow; upload corpus + crash
  artefacts; integrate crash triage into our bug tracker.
- Out: structured grammar-guided fuzzing (future).

## Deliverables
- `.github/workflows/fuzz-nightly.yml` with a 24 h budget.
- Slack / email hook for crash alerts.

## Acceptance
- Two consecutive nightly runs with 0 new unique crashes.

## References
- Plan §8.5.
