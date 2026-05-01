# 05-41: Parser blocker xfail shrink

**Phase:** 05-parse    **Depends on:** 05-31 through 05-40    **Milestone:** M5/M6 gate

## Goal
Close the parser-owned xfail loop by removing or reclassifying every
c-testsuite parse xfail that was caused by parser syntax.

## Scope
- In:
  - Re-run the full c-testsuite parse smoke.
  - Remove xfail entries that now pass due to 05-31 through 05-40.
  - Reclassify remaining failures as preprocessor, headers/builtin-rt,
    HIR/typeck, C11-only, or GNU-extension-semantic work.
  - Add reduced fixtures for any parser-owned failure that remains.
- Out:
  - Executing tests or raising runtime conformance percentages.

## Deliverables
- Updated `third_party/testsuites/c-testsuite/xfail.toml`.
- Parser smoke output summarized in the task report.
- Reduced parser fixtures for any remaining syntax issue.
- `docs/parser-feature-matrix.md` update.

## Acceptance
- `cargo test -p rcc_parse ctestsuite_parse_smoke --test ctestsuite_smoke -- --nocapture`
  reports zero unexpected failures and zero unexpected passes.
- No xfail reason says "parser limitation" without naming a concrete
  parser task or a non-parser owner.
- Every remaining parse xfail has an owner phase.

## References
- `crates/rcc_parse/tests/ctestsuite_smoke.rs`.
- `third_party/testsuites/c-testsuite/xfail.toml`.
- Tasks 05-31 through 05-40.
