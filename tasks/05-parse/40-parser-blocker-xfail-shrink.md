# 05-40: Parser blocker xfail shrink

> ✓ done — 2026-05-01

**Phase:** 05-parse    **Depends on:** 05-31 through 05-39    **Milestone:** M5/M6 gate

## Goal
Close the parser-owned xfail loop by removing or reclassifying every
c-testsuite parse xfail that was caused by parser syntax.

## Scope
- In:
  - Re-run the full c-testsuite parse smoke.
  - Remove xfail entries that now pass due to 05-31 through 05-39.
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

## Result
- Parser-owned GNU attribute syntax gap from `c-testsuite::00210` fixed
  with reduced fixtures for attributes before a base type and inside an
  abstract pointer declarator.
- `c-testsuite::00210` removed from `xfail.toml`.
- Remaining xfails: 211/220 passed, 9 xfail, 0 unexpected failures, 0
  unexpected passes.
- Remaining owner buckets:
  - `04-preprocess`: macro-expanded `#line`.
  - `06-hir-lower`: aggregate/initializer lowering details.
  - `14-lang-extensions`: anonymous/empty aggregate extensions.
  - `15-builtin-rt`: freestanding standard headers.

## References
- `crates/rcc_parse/tests/ctestsuite_smoke.rs`.
- `third_party/testsuites/c-testsuite/xfail.toml`.
- Tasks 05-31 through 05-39.
