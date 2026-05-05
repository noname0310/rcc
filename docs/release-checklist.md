# Release Checklist

This checklist is the ordered local/CI gate sequence for an M7 release
candidate. Commands marked "manual" are intentionally not required on every
local machine because they need network, GitHub Actions, crates.io auth, GPL
suites, or a long-running fuzz/differential budget.

## Local Required Gates

Run from the repository root unless noted.

| # | Command | Status / owner | Notes |
|---|---------|-----------------------|-------|
| 0 | `cargo xtask release-check` | task 13-13 | Runs the local release-candidate gate suite, writes logs under `reports/release-check/`, and reports actionable skips for missing optional tools. It checks the publish-only wrapper without default features everywhere, and checks default LLVM install features when LLVM 18 is configured. Add `--registry-package` only after task 13-14 makes internal crates registry-resolvable. |
| 1 | `git status --short` | run | Worktree audit before release commands. |
| 2 | `cargo fmt --all --check` | run | Formatting gate. |
| 3 | `cargo clippy --workspace --all-targets -- -D warnings` | run | Full workspace lint gate. |
| 4 | `cargo test --workspace` | run | Default no-LLVM workspace gate. |
| 5 | `LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 cargo test --workspace --features rcc_codegen_llvm/llvm` | manual: LLVM Linux/WSL gate | Required in CI/release environment; local Windows hosts use the Windows LLVM-C command below. |
| 6 | `cargo xtask coverage --lcov lcov.info --json target/coverage/coverage-summary.json` | manual: expensive coverage gate | Required in CI; writes LCOV/JSON/text artifacts and enforces thresholds. |
| 7 | `cd fuzz && cargo +nightly fuzz run lex --target x86_64-unknown-linux-gnu -- -max_total_time=30` | manual: nightly fuzz gate | Mandatory CI smoke; local execution requires cargo-fuzz/nightly. |

## Release Conformance Gates

| # | Command | Status / owner | Notes |
|---|---------|-----------------------|-------|
| 8 | `cargo xtask fetch-testsuites` | manual: network fetch | Fetches permissive suites. CI runs this before test/conformance jobs. |
| 9 | `cargo xtask fetch-testsuites --include-gpl --only tcc-tests2` | manual: network fetch | Fetches GPL test sources into the runner/local cache for execution only. |
| 10 | `LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 cargo build --release -p rcc_driver --bin rcc --features rcc_codegen_llvm/llvm` | run in WSL for tasks 13-10/13-11 | Builds the release driver used by conformance, benchmarks, and hello-world smoke. |
| 11 | `cargo run --release --package rcc_conformance --bin rcc_conformance_run -- --rcc target/release/rcc --suite c-testsuite --suite chibicc --suite tcc-tests2 --suite llvm-test-suite --mode stage-1-3 --output docs/conformance.json` | manual: release dashboard refresh | Required before final tag when dashboard data changes. |
| 12 | `cargo run --release --package rcc_conformance --bin rcc_conformance_render -- --input docs/conformance.json --output docs/conformance.md` | manual: release dashboard refresh | Regenerates the checked-in dashboard. |
| 13 | `python scripts/ci/check_kpi.py` | manual: depends on refreshed dashboard | Enforces zero non-xfailed failures and M7 thresholds. |

## Platform Smoke

| # | Command | Status / owner | Notes |
|---|---------|-----------------------|-------|
| 14 | `cargo run --bin rcc -- --version --verbose` | run | Info-only tool discovery; does not require input. |
| 15 | `cargo run --bin rcc -- --print-search-dirs` | run | Reports PATH search dirs and selected/missing tools. |
| 16 | `LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 target/release/rcc /tmp/rcc-platform-hello.c -O2 -o /tmp/rcc-platform-hello && /tmp/rcc-platform-hello` | run in WSL | Linux/WSL link+run hello-world smoke. |
| 17 | `$env:LLVM_SYS_181_PREFIX='D:\Tools\clang+llvm-18.1.8-x86_64-pc-windows-msvc'; $env:Path="$env:LLVM_SYS_181_PREFIX\bin;$env:Path"; cargo test -p rcc_codegen_llvm --features llvm-windows-llvm-c --test llvm_ir_snapshots -- --test-threads=1` | manual: Windows LLVM-C setup | Validates Windows host LLVM-C import library/dll layout; does not imply Windows target support. |

## Performance And Fuzz Extensions

| # | Command | Status / owner | Notes |
|---|---------|-----------------------|-------|
| 18 | `cargo bench -p rcc_lexer --bench lex -- --test` | run in task 13-10 | Fast Criterion compile check. |
| 19 | `cargo bench -p rcc_preprocess --bench preprocess -- --test` | run in task 13-10 | Fast Criterion compile check. |
| 20 | `cargo bench -p rcc_parse --bench parse -- --test` | run in task 13-10 | Fast Criterion compile check. |
| 21 | `cargo bench -p rcc_driver --bench pipeline -- --test` | run in task 13-10 | Fast Criterion compile check. |
| 22 | `RCC_BENCH_DATE=2026-05-05 RCC_BENCH_HOST="$(uname -srmo)" cargo xtask bench-runtime --rcc target/release/rcc --host-cc cc --iterations 3 --out docs/perf-baseline.md` | run in WSL in task 13-10 | Generates the checked-in runtime baseline. |
| 23 | `gh workflow run fuzz-lex-30m.yml -f max_total_time=1800 -f max_len=131072` | manual: GitHub Actions budget | Path-filtered extended fuzz; run before release or when lexer changes. |
| 24 | `gh workflow run fuzz-preprocess-30m.yml -f max_total_time=1800 -f max_len=131072` | manual: GitHub Actions budget | Path-filtered extended fuzz; run before release or when preprocessor changes. |
| 25 | `gh workflow run fuzz-parse-30m.yml -f max_total_time=1800 -f max_len=131072` | manual: GitHub Actions budget | Path-filtered extended fuzz; run before release or when parser changes. |
| 26 | `gh workflow run csmith-bounded.yml -f max_duration_secs=300 -f iterations=10000` | manual exploratory | Bug-discovery workflow. Failures create follow-up tasks; they are not hidden by release gates. |

## Release Packaging

Task 13-14 owns the final release workflow. Its intended local/auth-sensitive
steps are listed here for order only:

| # | Command | Status / owner | Notes |
|---|---------|-----------------------|-------|
| 27 | `cargo xtask release-check --registry-package` | manual after internal publish graph exists | Runs the crates.io-facing package archive check for the publish-only `rcc-compiler` package. Before task 13-14 publishes or otherwise resolves internal crates, release-check reports this as an explicit skip. The package default feature enables LLVM so plain `cargo install rcc-compiler` produces a real compiler binary. |
| 28 | `cargo publish --manifest-path crates/rcc_compiler_package/Cargo.toml --dry-run` | manual: crates.io dry-run | Auth/network-sensitive final dry-run for `cargo install rcc-compiler`; task 13-14 owns real publish. |
| 29 | `gh run list --commit <SHA> --limit 20` | manual: after push | Confirms mandatory workflows for the exact release commit. |
| 30 | `gh release create <tag> ...` | manual: final release | The workflow should upload built binaries automatically; direct CLI use is fallback only. |

## Policy Reminders

- Required C99 conformance failures are compiler bugs, even if aggregate pass
  percentages look good.
- GNU/C11/TinyCC-specific cases must be explicitly xfailed or tracked as
  extension tasks; they are not part of the strict C99 release gate.
- Windows host support and Windows target support are separate. The M7
  release target remains `x86_64-unknown-linux-gnu`.
- `rcc` uses hosted libc and external LLVM/system linker tools; it does not
  ship libc or a native linker.
