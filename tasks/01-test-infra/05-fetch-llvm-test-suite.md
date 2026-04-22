# 01-05: Vendor llvm-test-suite SingleSource

**Phase:** 01-test-infra    **Depends on:** 01-01    **Milestone:** M0.5

## Goal
Fetch only `SingleSource/UnitTests/` from llvm-test-suite. The rest of
the repo is multi-gigabyte and irrelevant to a C99 frontend.

## Scope
- In: sparse list limited to `SingleSource/UnitTests` + `LICENSE.txt` +
  top-level `CMakeLists.txt` (for metadata); copy license into
  `LICENSES/llvm-test-suite.txt`.
- Out: actually running the CMake tests (replaced with a bespoke
  adapter later).

## Deliverables
- Manifest entry pinned to an LLVM release branch tip.
- Sparse checkout is verified (clone size under 30 MB).

## Acceptance
- `cargo xtask fetch-testsuites --only llvm-test-suite` runs to
  completion; cloned repo size < 30 MB.
- `ls third_party/testsuites/llvm-test-suite/SingleSource/UnitTests/`
  contains at least one `.c` file.

## References
- Plan §9.1 "LLVM test-suite".
- Upstream: https://github.com/llvm/llvm-test-suite
