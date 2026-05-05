# 13-09: Mandatory CI green matrix

> ✓ done — 2026-05-05

**Phase:** 13-quality    **Depends on:** 13-08    **Milestone:** M7

## Goal
Define which GitHub Actions jobs are mandatory for release and make failures
actionable. Manual long-running jobs can remain manual, but their exclusion
must be explicit and documented.

## Scope
- In:
  - Audit `.github/workflows/*.yml` for duplicate setup, missing suite fetches,
    wrong path filters, and jobs that pass only because tests are skipped.
  - Make mandatory jobs green: fmt, clippy, no-LLVM tests, LLVM tests,
    coverage, conformance, fuzz smoke, path-filtered 30m fuzz gates.
  - Keep full gcc-torture and large llvm-test-suite runs manual unless release
    policy promotes them.
  - Add `gh run` commands to docs so local status checks are reproducible.
- Out:
  - Branch protection configuration that cannot be represented in the repo.

## Deliverables
- Updated workflow YAML if needed.
- `docs/ci.md` listing mandatory, optional, and manual jobs.
- Any CI-only test skips either removed or justified in docs.

## Acceptance
- Latest push to `main` has all mandatory jobs green.
- Coverage and LLVM jobs fetch the same optional suites required by their
  tests.
- No mandatory job masks a compiler bug with a broad skip.

## References
- `.github/workflows/ci.yml`.
- `.github/workflows/fuzz-lex-30m.yml`.
- `.github/workflows/fuzz-preprocess-30m.yml`.
- `.github/workflows/fuzz-parse-30m.yml`.
