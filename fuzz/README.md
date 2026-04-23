# rcc fuzz workspace

This directory is a **sub-workspace** (see the top-level `Cargo.toml`
`exclude = ["fuzz"]` and `fuzz/Cargo.toml`'s own empty `[workspace]`
table). It exists so that `cargo-fuzz` can pull nightly-only features
without infecting the main workspace.

Targets:

| target       | entry point                           | purpose                                    |
|--------------|---------------------------------------|--------------------------------------------|
| `lex`        | `fuzz_targets/lex.rs`                 | 24 h no-panic gate for `rcc_lexer` (M1)   |
| `preprocess` | `fuzz_targets/preprocess.rs`          | phase 04 — wired ahead of time            |

## Prerequisites

- `cargo-fuzz` ≥ 0.13:
  ```bash
  cargo install cargo-fuzz
  ```
- A Rust **nightly** toolchain. libFuzzer support and sanitiser flags
  require nightly today. Any reasonably recent nightly works; CI pins
  a specific date, local use does not need to.

## Seed corpus

A curated subset of small `.c` files from
`third_party/testsuites/c-testsuite/tests/single-exec/` lives in
`corpus/lex/`. These seeds are checked in so fresh clones can run the
fuzzer without a vendored suite.

To refresh the corpus from a freshly fetched testsuite, run one of:

```bash
# Linux / macOS / CI
./scripts/fuzz/seed-lex.sh
```

```powershell
# Windows local dev
powershell -ExecutionPolicy Bypass -File scripts/fuzz/seed-lex.ps1
```

The scripts are idempotent — they overwrite existing seed files with
the same curated set.

## Running the lex fuzzer

From the **repository root** (not `fuzz/`):

```bash
cargo +nightly fuzz run lex
```

`cargo-fuzz` automatically adds the seeds in `fuzz/corpus/lex/` to the
initial queue. Default options come from `fuzz/.cargo/config.toml`
aliases:

```bash
# Inside fuzz/ directory — aliases resolve via .cargo/config.toml.
cd fuzz
cargo +nightly fuzz-lex         # ASAN + libFuzzer defaults
cargo +nightly fuzz-lex-nosan   # Windows / MSVC fallback
```

Or pass the flags directly:

```bash
cargo +nightly fuzz run lex --sanitizer=address -- \
    -max_len=131072 -rss_limit_mb=4096
```

### CI smoke run (≤ 30 s)

```bash
cargo +nightly fuzz run lex -- -max_total_time=30 -max_len=131072
```

Exit code is non-zero iff libFuzzer reported a crash, slow unit, or
leak — that is the acceptance gate.

### Local milestone run (≥ 10 min, up to 24 h)

```bash
cargo +nightly fuzz run lex -- -max_total_time=600 -max_len=131072   # 10 min
cargo +nightly fuzz run lex -- -max_total_time=86400 -max_len=131072 # 24 h
```

Any discovered crash is written to `fuzz/artifacts/lex/` and should be
minified (`cargo +nightly fuzz tmin lex <artifact>`) and then filed as
a new task under 03-lex/ or the relevant downstream phase.

## Windows caveats

libFuzzer on Windows targets MSVC's `clang.exe` runtime. In practice,
on stock `nightly-x86_64-pc-windows-msvc`:

- `cargo +nightly fuzz build lex` may fail with link-time errors
  referring to missing `libfuzzer.lib` / ASAN runtime. This is an
  upstream toolchain gap, not an `rcc` issue.
- When that happens, the recommended workflow is:
  1. Sanity-check the target compiles via
     `cargo check -p rcc-fuzz --manifest-path fuzz/Cargo.toml`
     (no libFuzzer linkage).
  2. Run the *corpus* invariants test instead:
     `cargo test -p rcc_lexer --test corpus`. It exercises every
     seed input through the lexer with the same "no panics, no
     cross-token gaps" contract.
  3. Run the actual fuzzer on Linux / macOS (CI or WSL).

CI runs on Linux and enforces the real fuzz-based gates; Windows is a
best-effort local environment for this particular task.

## References

- Task spec: [`tasks/03-lex/12-fuzz-target.md`](../tasks/03-lex/12-fuzz-target.md).
- cargo-fuzz book: <https://rust-fuzz.github.io/book/cargo-fuzz.html>.
- libFuzzer flags: <https://llvm.org/docs/LibFuzzer.html#options>.
