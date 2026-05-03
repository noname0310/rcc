> ✓ done — 2026-05-04

# 01-12: Seed empty `xfail.toml` per suite

**Phase:** 01-test-infra    **Depends on:** 01-08, 01-09    **Milestone:** M0.5

## Goal
Ship an empty (`xfail = []`) `xfail.toml` alongside each vendored
suite so the harness has a consistent file to read. Later tasks add
entries pointing at the task id that will close the gap.

## Scope
- In: `third_party/testsuites/<suite>/xfail.toml` for every suite;
  header comment explains the schema and references
  [`tasks/00-overview/03-working-agreement.md`](../00-overview/03-working-agreement.md).
- Out: actual expected-failure entries (added by feature tasks).

## Deliverables
- 6 new `xfail.toml` files (one per suite).
- `rcc_conformance::xfail::load` returns `XFailFile::default()`
  cleanly for empty files — regression test added.

## Acceptance
- `cargo test -p rcc_conformance --test xfail_roundtrip` green.
- Running the conformance binary with no xfail entries shows 0
  `XFail` outcomes but does not error.

## References
- Plan §9.3.
