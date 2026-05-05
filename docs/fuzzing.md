# Fuzzing workflow

This project uses `cargo-fuzz` for lexer, preprocessor, and parser no-crash
testing. The long-running workflows are intentionally path-filtered 30-minute
jobs, not scheduled cron jobs.

## Fetch a GitHub Actions artifact

From a failed run, download the uploaded corpus/crash artifact:

```bash
gh run download <RUN_ID> --name preprocess-fuzz --dir target/fuzz-artifacts
```

Artifact names:

| Target | Workflow | Artifact |
|--------|----------|----------|
| `lex` | `.github/workflows/fuzz-lex-30m.yml` | `lex-fuzz-shard-0` |
| `preprocess` | `.github/workflows/fuzz-preprocess-30m.yml` | `preprocess-fuzz` |
| `parse` | `.github/workflows/fuzz-parse-30m.yml` | `parse-fuzz` |

The workflows upload both `fuzz/corpus/<target>/` and
`fuzz/artifacts/<target>/` with `if: always()`, so the artifact is present on
success and failure.

## Reproduce

Use the exact command printed by libFuzzer. For example:

```bash
cd fuzz
cargo +nightly fuzz run preprocess artifacts/preprocess/crash-492ca4dbfea75efcbd100c5b0c994580a7518b75
```

If the artifact was downloaded outside `fuzz/`, pass the relative or absolute
path directly:

```bash
cd fuzz
cargo +nightly fuzz run preprocess ../target/fuzz-artifacts/preprocess-fuzz/artifacts/preprocess/crash-...
```

## Minimize

Minimize before promotion when the artifact is large or hard to review:

```bash
cd fuzz
cargo +nightly fuzz tmin preprocess artifacts/preprocess/crash-...
```

Commit the minimized input only after the underlying bug is understood and the
current head no longer crashes on it.

## Promote a reviewed crash

After fixing the bug, promote the reviewed artifact into the curated corpus:

```bash
cargo xtask fuzz-regression preprocess \
    target/fuzz-artifacts/preprocess-fuzz/artifacts/preprocess/crash-... \
    --name preprocess-recursive-include.rccfuzz
```

The helper copies the artifact to `fuzz/corpus/<target>/` and prints:

- a one-shot reproduce command against the promoted corpus input;
- a `cargo fuzz tmin` command if further minimization is useful.

`fuzz/corpus/*` should contain intentional seeds only. Do not commit raw crash
spam from `fuzz/artifacts/*`; those directories stay ignored.

## Known crash regression links

| Crash | Permanent regression |
|-------|----------------------|
| Preprocessor recursive virtual include stack overflow (`crash-492ca4db...`) | `fuzz/corpus/preprocess/preprocess-recursive-include.rccfuzz` plus `rcc_preprocess::include::tests::recursive_virtual_include_is_diagnosed_without_stack_overflow` |
