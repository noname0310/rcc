# Publishing

`rcc` is published to crates.io as package **`rcc-compiler`** because the crate
name `rcc` is already taken. The installed binary remains `rcc`:

```bash
cargo install rcc-compiler
rcc --version
```

## Package Graph

`rcc-compiler` is a publish-only wrapper crate under
`crates/rcc_compiler_package/`. It is intentionally excluded from the workspace
so normal development commands keep using `rcc_driver`'s `rcc` binary without an
ambiguous second workspace binary.

The release publish order is encoded in `xtask::release_publish::PUBLISH_ORDER`:

```text
rcc_span -> rcc_target -> rcc_data_structures -> rcc_errors -> rcc_session
-> rcc_ast -> rcc_hir -> rcc_lexer -> rcc_preprocess -> rcc_parse
-> rcc_typeck -> rcc_hir_lower -> rcc_cfg -> rcc_cfg_transform
-> rcc_codegen_llvm -> rcc_driver -> rcc-compiler
```

Internal crates use `{ version = "...", path = "..." }` dependencies so local
development uses paths while crates.io consumers resolve versions.

## Authentication

Local publishing uses `cargo login` credentials. CI publishing uses the
repository secret `CRATE_TOKEN`:

```bash
cargo xtask release-publish --token-env CRATE_TOKEN
```

Never write the token into a file or command log.

## Local Commands

Dry-run the release version calculation:

```bash
cargo xtask release-bump patch --dry-run
```

Publish the crate graph from a logged-in local machine:

```bash
cargo xtask release-publish
```

After publishing, verify installability:

```bash
cargo install rcc-compiler --version 0.0.0 --force
rcc --version --verbose
```

## Recovery

If `cargo publish` fails before `rcc-compiler` is uploaded, fix the failing
crate and rerun the publish command. Already uploaded crate versions cannot be
overwritten.

If a GitHub Release or tag exists but crates.io publish failed:

- delete the draft release if it has not been announced,
- delete the tag if no published crate points at it yet,
- otherwise publish a follow-up patch version and document the failed attempt.

If a bad crate version was published, prefer a corrective patch release. Yank
only when the uploaded version is actively harmful:

```bash
cargo yank --vers <version> <crate-name>
```
