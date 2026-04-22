> ✓ done — 2026-04-23

# 01-06: Vendor + build csmith

**Phase:** 01-test-infra    **Depends on:** 01-01    **Milestone:** M0.5

## Goal
Clone csmith, pin to a tag, and document the build command
(`cmake -S . -B build && cmake --build build`) used by the
differential runner in phase 12. We do **not** build csmith at
`fetch-testsuites` time (it has its own cmake build); we only place
sources and write an `INSTALL.md` into the directory.

## Scope
- In: manifest entry; BSD license copy; post-fetch `INSTALL.md` noting
  the host tools needed (`cmake`, `m4`).
- Out: actual differential runs (phase 12).

## Deliverables
- `third_party/testsuites/csmith/INSTALL.md` with build commands.
- `LICENSES/csmith.txt`.

## Acceptance
- `cargo xtask fetch-testsuites --only csmith` completes.
- `third_party/testsuites/csmith/CMakeLists.txt` is present.
- A follow-up manual `cmake -B build && cmake --build build` succeeds
  on the CI runner (smoke-tested in [`12-fuzz-differential/04-csmith-differential-harness.md`](../12-fuzz-differential/04-csmith-differential-harness.md)).

## References
- Plan §9.1 "Csmith".
- Upstream: https://github.com/csmith-project/csmith
