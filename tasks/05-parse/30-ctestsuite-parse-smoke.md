> ✓ done — 2026-04-24

# 05-30: c-testsuite parse smoke

**Phase:** 05-parse    **Depends on:** 05-28, 01-01    **Milestone:** M2

## Goal
Run the full preprocessor + parser pipeline over every file in
`third_party/testsuites/c-testsuite/tests/single-exec/`. Every file
must return `Some(TranslationUnit)` from `rcc_parse::parse`. Pass
count populates `docs/conformance.md`'s c-testsuite row at the
"parsed" granularity.

## Scope
- In: dedicated test `crates/rcc_parse/tests/ctestsuite_smoke.rs`
  gated on suite presence; log which file (if any) fails.
- Out: executing the programs (that's M3 work).

## Deliverables
- Smoke test file.
- Diagnostic bundle on failure (print the first diagnostic's span).

## Acceptance
- `cargo test -p rcc_parse --test ctestsuite_smoke`: green on all
  non-`xfail`ed files.
- xfail entries added to `third_party/testsuites/c-testsuite/xfail.toml`
  with reason pointing at the feature task that will close the gap.

## References
- Plan §10 M2.
