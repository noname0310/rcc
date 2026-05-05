# 13-13: Release candidate dry run

> ✓ done — 2026-05-05

**Phase:** 13-quality    **Depends on:** 13-12    **Milestone:** M7

## Goal
Create one local command that answers "is this commit releasable?" before a
tag is pushed.

## Scope
- In:
  - Add `cargo xtask release-check` or `scripts/release-check.*`.
  - Run fmt, clippy, no-LLVM tests, LLVM tests when LLVM is available,
    coverage, conformance dashboard refresh, mandatory fuzz smoke, and package
    checks.
  - Run `cargo publish --dry-run` / `cargo package` for the publishable crate
    set needed by `cargo install rcc-compiler`.
  - Check that the publish package name is `rcc-compiler` and the installed
    binary name is `rcc`.
  - Check that plain `cargo install rcc-compiler` would enable the LLVM backend
    by default; a no-LLVM wrapper check is only for partially provisioned hosts.
  - For the selected publish-only package strategy, treat registry packaging as
    a separate `--registry-package` gate until task 13-14 makes internal crates
    registry-resolvable.
  - Print skipped gates with explicit reasons instead of silently ignoring
    missing tools.
  - Save logs under `reports/release-check/` and ignore that directory in git.
- Out:
  - Creating the GitHub Release.
  - Pushing tags.

## Deliverables
- Release-check command.
- `docs/release-checklist.md` updated to use it.
- Git ignore entry for local release reports.

## Acceptance
- On a fully provisioned Linux/WSL machine, the release check completes
  without manual edits.
- On a partially provisioned machine, missing LLVM/cargo-fuzz/toolchain
  prerequisites are reported as actionable skips or hard failures according to
  the documented policy.
- The command exits non-zero if any mandatory gate fails.
- The command exits non-zero if local `cargo install rcc-compiler` structure is
  broken: missing `rcc-compiler` package, missing `rcc` binary, or a wrapper
  that cannot build against `rcc_driver`.
- The command validates the wrapper without default features everywhere and
  validates default LLVM install features when an LLVM 18 prefix is configured.
- The command documents the crates.io archive dry-run as skipped unless
  `--registry-package` is used after the internal crate publish graph exists.

## References
- `xtask/`.
- `docs/ci.md`.
- `docs/platform-support.md`.
