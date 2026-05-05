# CI matrix

The release branch uses commit-based review, but GitHub Actions still defines
the mandatory release checks. A job is mandatory when it runs automatically on
`push` to `main` or on the path-filtered extended workflows below.

## Mandatory jobs

| Workflow | Job | Trigger | Purpose |
|----------|-----|---------|---------|
| `CI` | `rustfmt` | push, PR, manual | Formatting gate. |
| `CI` | `clippy` | push, PR, manual | `cargo clippy --workspace --all-targets -- -D warnings`. |
| `CI` | `test (no-llvm)` | push, PR, manual | Default workspace tests with required vendored suites fetched. |
| `CI` | `test (llvm)` | push, PR, manual | Workspace tests with LLVM 18 and `rcc_codegen_llvm/llvm`. |
| `CI` | `coverage (llvm-cov)` | push, PR, manual | `cargo xtask coverage` threshold gate and artifacts. |
| `CI` | `fuzz smoke (lexer)` | push, PR, manual | 30-second lexer libFuzzer smoke, with crash artifacts uploaded. |
| `CI` | `conformance (release dashboard)` | push, PR, manual | M7 release dashboard: c-testsuite, chibicc stage-1-3, tcc-tests2, llvm-test-suite. |
| `CI` | `gcc-torture smoke` | push, PR, manual | Curated GCC torture smoke subset with LLVM 18. |
| `Fuzz Lex 30m` | `lex 30m` | path-filtered push, manual | Extended lexer fuzz gate. |
| `Fuzz Preprocess 30m` | `preprocess 30m` | path-filtered push, manual | Extended preprocessor fuzz gate. |
| `Fuzz Parse 30m` | `parse 30m` | path-filtered push, manual | Extended parser fuzz gate. |

Mandatory jobs must fail loudly. Do not add `continue-on-error`, broad skips,
or xfail-style demotions to these jobs to hide compiler bugs. If a job exposes
a real compiler failure, fix the compiler or move the job out of the mandatory
set with a documented policy reason.

## Manual exploratory jobs

| Workflow | Trigger | Why manual |
|----------|---------|------------|
| `Csmith Bounded Differential (manual)` | `workflow_dispatch` | Generates broad random programs and currently finds many real bugs; useful for bug discovery, not a deterministic release gate. |
| `llvm-test-suite SingleSource (manual)` | `workflow_dispatch` job inside `CI` | The curated release subset already runs in the conformance dashboard; this job is for manual expansion/debugging. |
| `gcc-torture execute (manual, long-running)` | `workflow_dispatch` job inside `CI` | Full GCC torture execute is long-running and broader than the first C99 release gate. |

Manual jobs must still upload reports. Their failures should become concrete
tasks, not silent skips.

## Local equivalents

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Linux / WSL with LLVM 18
LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 \
  cargo test --workspace --features rcc_codegen_llvm/llvm

cargo xtask coverage --lcov lcov.info --json target/coverage/coverage-summary.json

cd fuzz
cargo +nightly fuzz run lex --target x86_64-unknown-linux-gnu -- -max_total_time=30
```

Release conformance locally:

```bash
cargo xtask fetch-testsuites
cargo xtask fetch-testsuites --include-gpl --only tcc-tests2
LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 \
  cargo build --release -p rcc_driver --bin rcc --features rcc_codegen_llvm/llvm
cargo run --release --package rcc_conformance --bin rcc_conformance_run -- \
  --rcc target/release/rcc \
  --suite c-testsuite \
  --suite chibicc \
  --suite tcc-tests2 \
  --suite llvm-test-suite \
  --mode stage-1-3 \
  --output docs/conformance.json
python scripts/ci/check_kpi.py
```

## GitHub CLI checks

Inspect the newest runs for the current branch:

```bash
gh run list --branch main --limit 20
gh run view <RUN_ID> --log-failed
gh run watch <RUN_ID>
```

Inspect a specific commit after pushing:

```bash
git rev-parse HEAD
gh run list --commit <SHA> --limit 20
```

Manually run extended or exploratory jobs:

```bash
gh workflow run fuzz-lex-30m.yml -f max_total_time=60 -f max_len=131072
gh workflow run fuzz-preprocess-30m.yml -f max_total_time=60 -f max_len=131072
gh workflow run fuzz-parse-30m.yml -f max_total_time=60 -f max_len=131072
gh workflow run csmith-bounded.yml -f max_duration_secs=300 -f iterations=10000
```

The latest remote push checked during task 13-09 was `b101e267` on
2026-05-04 UTC: `CI` and the three 30-minute fuzz workflows were green, while
the old push-triggered csmith workflow failed with thousands of differential
failures. That workflow is now manual so mandatory release checks do not fail
on known exploratory bug-finding output.
