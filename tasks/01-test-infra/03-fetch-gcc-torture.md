> ✓ done — 2026-04-23

# 01-03: Vendor gcc-torture (optional external suite)

**Phase:** 01-test-infra    **Depends on:** 01-01    **Milestone:** M0.5

## Goal
Wire opt-in fetch of `gcc/testsuite/gcc.c-torture/{compile,execute}/`.
The opt-in boundary is enforced by `xtask`: without `--include-gpl` the
suite is skipped with a visible warning so ordinary fetches do not pull a
large optional suite.

## Scope
- In: verify `--include-gpl` flag; sparse path; `LICENSES/gcc-torture.txt`;
  an extra note in `LICENSES/README.md` stating these tests are run as
  separate processes and never linked.
- Out: adapter implementation (task under 11-conformance).

## Deliverables
- Manifest entry locked to a GCC release tag with `gpl = true`.
- README in `third_party/testsuites/gcc-torture/` (auto-written) that
  names the upstream license and warns "do not copy sources into this
  repo".

## Acceptance
- `cargo xtask fetch-testsuites` **without** `--include-gpl`: skips
  with a printed optional-suite message.
- `cargo xtask fetch-testsuites --include-gpl`: populates
  `third_party/testsuites/gcc-torture/gcc/testsuite/gcc.c-torture/execute/0000.c`
  (or similar first file).
- `git` pointer file matches the pinned tag.

## References
- Plan §9.1 "GCC C torture tests".
- GCC upstream: https://gcc.gnu.org/git/gcc.git
