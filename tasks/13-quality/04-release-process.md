# 13-04: Release process

**Phase:** 13-quality    **Depends on:** 13-01, 13-02, 13-03    **Milestone:** M7

## Goal
Cut a tagged `v0.1.0`. Includes:
- CHANGELOG.md with per-milestone notes.
- `cargo publish` dry-run for every library crate (crates-io name
  availability check).
- A signed git tag + GitHub Release with built binaries for
  Linux / macOS / Windows.

## Scope
- In: release workflow `.github/workflows/release.yml`; tag triggers
  it.
- Out: automated publishing to crates.io (manual for now until the
  compiler is stable).

## Deliverables
- `CHANGELOG.md`.
- Release workflow.
- `docs/release-notes-v0.1.0.md`.

## Acceptance
- Pushing tag `v0.1.0` yields:
  - Binaries attached to the release page.
  - `CHANGELOG.md` links.
  - `docs/conformance.md` snapshot captured in the release notes.

## References
- rustc release process.
- semver.org.
