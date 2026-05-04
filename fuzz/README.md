# rcc fuzz workspace

This directory is a **sub-workspace** (see the top-level `Cargo.toml`
`exclude = ["fuzz"]` and `fuzz/Cargo.toml`'s own empty `[workspace]`
table). It exists so that `cargo-fuzz` can pull Rust nightly toolchain
features without infecting the main workspace.

Targets:

| target       | entry point                           | purpose                                         |
|--------------|---------------------------------------|-------------------------------------------------|
| `lex`        | `fuzz_targets/lex.rs`                 | 30 minute path-filtered no-panic gate for `rcc_lexer` |
| `preprocess` | `fuzz_targets/preprocess.rs`          | `Session` + `Preprocessor::run` pipeline (M5)  |

## Prerequisites

- `cargo-fuzz` ≥ 0.13:
  ```bash
  cargo install cargo-fuzz
  ```
- A Rust **nightly** toolchain. libFuzzer support and sanitiser flags
  require nightly today. Any reasonably recent nightly works; CI pins
  a specific date, local use does not need to.

## Seed corpora

Each target has its own curated seed directory under `corpus/`:

| target       | seed dir              | source                                                             |
|--------------|-----------------------|--------------------------------------------------------------------|
| `lex`        | `corpus/lex/`         | `third_party/testsuites/c-testsuite/tests/single-exec/`            |
| `preprocess` | `corpus/preprocess/`  | `third_party/testsuites/chibicc/test/` (preprocessor-heavy inputs) |

Both sets are checked in so fresh clones can run the fuzzer without a
vendored suite. To refresh from a freshly fetched testsuite, run the
matching script:

```bash
# Linux / macOS / CI
./scripts/fuzz/seed-lex.sh
./scripts/fuzz/seed-preprocess.sh
```

```powershell
# Windows local dev
powershell -ExecutionPolicy Bypass -File scripts/fuzz/seed-lex.ps1
powershell -ExecutionPolicy Bypass -File scripts/fuzz/seed-preprocess.ps1
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

### Local milestone run

```bash
cargo +nightly fuzz run lex -- -max_total_time=600 -max_len=131072   # 10 min
cargo +nightly fuzz run lex -- -max_total_time=1800 -max_len=131072  # 30 min
```

Any discovered crash is written to `fuzz/artifacts/lex/` and should be
minified (`cargo +nightly fuzz tmin lex <artifact>`) and then filed as
a new task under 03-lex/ or the relevant downstream phase.

### Extended 30 minute run

The extended workflow lives at `.github/workflows/fuzz-lex-30m.yml`.
It runs the existing `lex` target once for 30 minutes when lexer/fuzz
paths change, or when manually dispatched. This is the default
personal-project release gate: long enough to catch shallow lexer
regressions without running compute on an unconditional cron.

Manual dispatch accepts shorter values for development:

```bash
gh workflow run fuzz-lex-30m.yml -f max_total_time=60 -f max_len=131072
```

The workflow uploads `fuzz/corpus/lex/` and its crash directory.
Failures use the repository's normal GitHub Actions notifications and
uploaded artifacts; no external incident channel is assumed.

## Running the preprocess fuzzer

The `preprocess` target drives the complete phase 04 pipeline:

```text
String::from_utf8_lossy
 → Session::new(Options::default())
 → SourceMap::add_file("<fuzz>", src)
 → Preprocessor::run(file_id)
```

Non-UTF-8 byte sequences are *not* discarded: the target runs them
through `from_utf8_lossy` so every libFuzzer mutation reaches the
preprocessor. Invalid sequences become U+FFFD — that's fine, UTF-8
validation is out of scope here.

Invoke it from the repository root:

```bash
cargo +nightly fuzz run preprocess
```

or via the sub-workspace aliases:

```bash
cd fuzz
cargo +nightly fuzz-preprocess         # ASAN + libFuzzer defaults
cargo +nightly fuzz-preprocess-nosan   # Windows / MSVC fallback
```

The `-timeout=25` flag in those aliases is intentionally generous:
real preprocessor hangs (recursive-macro stack overflows, runaway
token-paste loops) are the bugs this target exists to surface, so we
don't want spurious timeout false positives from a cold `cargo fuzz`
rebuild.

### CI smoke run (60 s)

The acceptance gate for this target is a 60-second run with zero
crashes and zero timeouts:

```bash
cargo +nightly fuzz run preprocess -- \
    -max_total_time=60 -max_len=131072 -rss_limit_mb=4096 -timeout=25
```

Exit code is non-zero iff libFuzzer reports a crash, leak, or
timeout. That is the acceptance gate tracked under
`tasks/04-preprocess/19-fuzz-target.md`.

### Recursive-macro sanity check

As documented in the task's Acceptance, a manually-introduced bug of
the form

```c
#define A B
#define B A
A
```

must be caught within seconds. Drop such a file into
`fuzz/corpus/preprocess/` (do **not** check it in!) and confirm the
fuzzer reports it before proceeding to any longer runs.

### Local milestone run

```bash
cargo +nightly fuzz run preprocess -- -max_total_time=600 -max_len=131072   # 10 min
cargo +nightly fuzz run preprocess -- -max_total_time=1800 -max_len=131072  # 30 min
```

Crashes land in `fuzz/artifacts/preprocess/`. Minify with
`cargo +nightly fuzz tmin preprocess <artifact>` and file a follow-up
task under `tasks/04-preprocess/` or the relevant downstream phase.

## Windows caveats

libFuzzer on Windows targets MSVC's `clang.exe` runtime. In practice,
on stock `nightly-x86_64-pc-windows-msvc`:

- `cargo +nightly fuzz build lex` (and `... build preprocess`) may
  fail with link-time errors referring to missing `libfuzzer.lib` /
  ASAN runtime. This is an upstream toolchain gap, not an `rcc` issue.
- Even when `cargo +nightly fuzz build <target>` succeeds, the
  produced `.exe` may crash at startup with
  `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)` because the ASAN
  interceptor DLL (`clang_rt.asan_dynamic-x86_64.dll`) is not on
  `PATH`. A local workaround is to prepend the clang-rt lib directory
  bundled with the current rustc (roughly
  `...\rustup\toolchains\nightly-x86_64-pc-windows-msvc\lib\rustlib\x86_64-pc-windows-msvc\bin\`)
  — but the supported workflow remains running the real fuzzer on
  Linux / macOS.
- `cargo +nightly fuzz run <target> --sanitizer=none` does *not*
  rescue this: libFuzzer still pulls in SanitizerCoverage and the
  MSVC linker cannot resolve `__start___sancov_cntrs` /
  `__stop___sancov_pcs` on stable Windows tool-chains.
- When that happens, the recommended workflow is:
  1. Sanity-check the target compiles via
     `cargo check -p rcc-fuzz --manifest-path fuzz/Cargo.toml`
     (no libFuzzer linkage).
  2. Run the equivalent corpus invariants test instead:
     - for `lex`: `cargo test -p rcc_lexer --test corpus`.
     - for `preprocess`: `cargo test -p rcc_preprocess --test chibicc`
       — exercises every chibicc preprocessor fixture through
       `Preprocessor::run` with a "no panic, bounded diagnostics"
       contract, which is the same shape of guarantee the fuzzer
       enforces under mutation.
  3. Run the actual fuzzer on Linux / macOS (CI or WSL).

CI runs on Linux and enforces the real fuzz-based gates; Windows is a
best-effort local environment for this particular task.

## References

- Task spec (lex): [`tasks/03-lex/12-fuzz-target.md`](../tasks/03-lex/12-fuzz-target.md).
- Extended task spec: [`tasks/12-fuzz-differential/01-lexer-fuzz-30m.md`](../tasks/12-fuzz-differential/01-lexer-fuzz-30m.md).
- Task spec (preprocess): [`tasks/04-preprocess/19-fuzz-target.md`](../tasks/04-preprocess/19-fuzz-target.md).
- GitHub-hosted runner limits: <https://docs.github.com/en/actions/reference/usage-limits-for-self-hosted-runners>.
- cargo-fuzz book: <https://rust-fuzz.github.io/book/cargo-fuzz.html>.
- libFuzzer flags: <https://llvm.org/docs/LibFuzzer.html#options>.
