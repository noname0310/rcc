# 01-01: Vendor c-testsuite

**Phase:** 01-test-infra    **Depends on:** —    **Milestone:** M0.5

## Goal
Make `cargo xtask fetch-testsuites --only c-testsuite` idempotent and
reproducible. After it runs, `third_party/testsuites/c-testsuite/`
contains the pinned revision of the MIT-licensed c-testsuite corpus.

## Scope
- In: verify git clone + checkout sequence; handle re-run (fetch +
  checkout to pinned rev); copy `LICENSE` into `LICENSES/c-testsuite.txt`.
- Out: running or scoring tests (tasks 08/10).

## Deliverables
- `third_party/MANIFEST.toml` entry for `c-testsuite` uses a specific
  commit SHA (no branch name).
- `xtask/src/fetch.rs` handles already-cloned repos without error.
- `LICENSES/c-testsuite.txt` is auto-populated.

## Acceptance
- `cargo xtask fetch-testsuites --only c-testsuite` succeeds on:
  - A clean checkout.
  - A second run (should be a no-op + fast).
- `git -C third_party/testsuites/c-testsuite rev-parse HEAD` matches the
  `rev` in `MANIFEST.toml`.
- `ls third_party/testsuites/c-testsuite/tests/single-exec/00001.c`
  exists.

## References
- Plan §9.1 "c-testsuite".
- Upstream: https://github.com/c-testsuite/c-testsuite
