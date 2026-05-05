# 13-14: Release process

**Phase:** 13-quality    **Depends on:** 13-13    **Milestone:** M7

## Goal
Create an automated release workflow for the `rcc` compiler after the
release-candidate dry run passes.

The crates.io package name is **`rcc-compiler`** because `rcc` is already
taken. The installed binary remains **`rcc`**, so users install with:

```bash
cargo install rcc-compiler
rcc --version
```

The release workflow is triggered manually with one of `major`, `minor`, or
`patch`. It computes the next semver version from the current manifest/tag,
updates versioned files, creates a release commit and tag, builds release
binaries, publishes to crates.io, and uploads the binaries to GitHub Releases.

## Scope
- In:
  - `.github/workflows/release.yml` with `workflow_dispatch` input:
    `bump = major | minor | patch`.
  - A version-bump step that updates:
    - workspace/package versions,
    - publishable crate dependency versions,
    - `Cargo.lock`,
    - `CHANGELOG.md` / release notes placeholders.
  - A release commit authored as
    `noname0310 <hjnam2014@gmail.com>`.
  - An annotated or signed tag `vX.Y.Z`.
  - GitHub Release creation from the generated tag.
  - Release binary builds for supported host platforms from the platform
    matrix.
  - Binary upload to the GitHub Release.
  - `cargo publish` / `cargo publish --dry-run` for the publishable crates
    needed by `cargo install rcc-compiler`.
  - Release notes include the frozen conformance dashboard, xfail policy,
    supported platforms, and known non-goals.
- Out:
  - Publishing a package named `rcc`; that crate name is taken.
  - Claiming Windows target support if only the Windows host build works.
  - Implementing libc, glibc, MSVCRT, or a native linker as part of release.

## crates.io packaging policy

`cargo install rcc-compiler` is a hard requirement. The selected strategy is
**option 2: a publish-only distribution crate** (`crates/rcc_compiler_package`)
so the development workspace keeps the existing `rcc_driver` package name and
does not introduce a second workspace `rcc` binary.
The package's default feature enables the LLVM backend; a no-default-features
build is only a local wrapper sanity check for hosts without LLVM 18.

The rejected alternative remains documented for context:

1. **Publish the workspace crate graph.**
   - Rename the driver package to `rcc-compiler`, keep `[[bin]] name = "rcc"`.
   - Set `publish = true` for `rcc-compiler` and every internal `rcc_*`
     library crate it depends on.
   - Replace path-only internal dependencies with `{ version = "...", path =
     "..." }` so local development still works and crates.io gets resolvable
     versions.
   - Publish dependency crates in topological order before publishing
     `rcc-compiler`.

2. **Create a publish-only distribution crate.** **Selected.**
   - Add a crate such as `crates/rcc_compiler_package` with package name
     `rcc-compiler` and binary name `rcc`.
   - It may re-export/wrap internal crates, but every crates.io dependency must
     still be published or external.
   - This avoids renaming `rcc_driver`, but adds release packaging complexity.

## Deliverables
- `CHANGELOG.md`.
- Release workflow.
- `docs/release-notes-v0.1.0.md`.
- `docs/publishing.md` covering crates.io package ownership, required secrets,
  publish order, rollback/yank policy, and `cargo install rcc-compiler`.
- Optional helper script or `xtask release-bump --major|--minor|--patch` used
  by the workflow and runnable locally.

## Acceptance
- `workflow_dispatch` requires exactly one bump kind: `major`, `minor`, or
  `patch`.
- The workflow refuses to run unless task 13-13's release-check gate passes.
- A dry-run mode demonstrates the computed next version without pushing a
  commit, tag, release, or crates.io upload.
- A real run:
  - creates one release commit,
  - creates tag `vX.Y.Z`,
  - creates a GitHub Release,
  - uploads supported binaries,
  - publishes `rcc-compiler` so `cargo install rcc-compiler` installs an
    executable named `rcc`,
  - attaches `docs/conformance.md` / release notes snapshot.
- Release notes explicitly list unsupported targets and extension-heavy
  exploratory suites.
- If crates.io publish fails after the GitHub Release is created, the workflow
  must fail loudly and document the manual recovery steps (delete draft
  release, delete tag if unpublished, or yank published crates if needed).

## References
- semver.org.
- crates.io package publishing and yanking.
- GitHub Actions `workflow_dispatch`.
- `gh release create`.
