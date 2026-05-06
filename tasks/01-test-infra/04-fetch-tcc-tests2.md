> ✓ done — 2026-04-23

# 01-04: Vendor tcc tests2 (optional external suite)

**Phase:** 01-test-infra    **Depends on:** 01-01    **Milestone:** M0.5

## Goal
Fetch TinyCC's `tests/tests2/` directory behind the same `--include-gpl`
flag used for GCC torture. The flag is kept as an explicit external-suite
opt-in; CI may fetch and execute the suite inside the runner.

## Scope
- In: sparse path for `tests/tests2`; pin upstream to a tag; copy
  `COPYING` file into `LICENSES/tcc-tests2.txt`.
- Out: adapter (future task).

## Deliverables
- `MANIFEST.toml` entry pinned to a TCC release.
- `xtask` run produces `third_party/testsuites/tcc-tests2/tests/tests2/*.c`.

## Acceptance
- `cargo xtask fetch-testsuites --only tcc-tests2 --include-gpl` works
  both on first run and on re-run.
- Total file count matches the upstream tests2 directory (asserted via
  shell script in CI; numeric bound: ≥ 100 files).

## References
- Plan §9.1 "tcc 테스트".
- Upstream: https://github.com/TinyCC/tinycc.git
