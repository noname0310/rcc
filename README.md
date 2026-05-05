# rcc — a Rust-based C99 compiler

`rcc` is a C99 compiler written in Rust, targeting **LLVM IR** through
[`inkwell`](https://github.com/TheDan64/inkwell). Its architecture is
modelled after the `rustc` multi-crate workspace: each stage
(preprocess / lex / parse / HIR / typeck / MIR / codegen) lives in its
own crate and communicates through narrow public type boundaries.

> **Status.** `rcc` now has a working C99 front end, HIR/type checking,
> CFG lowering, LLVM IR/object emission, conformance adapters, fuzz targets,
> and release-quality gates. The M7 release target is hosted
> `x86_64-unknown-linux-gnu`; other parsed target triples are documented as
> layout/front-end models unless explicitly listed in
> [`docs/platform-support.md`](docs/platform-support.md).

## Repository layout

```
crates/
  rcc_span/             # Spans, SourceMap, Symbol interner
  rcc_errors/           # Diagnostic, Handler, Emitter
  rcc_target/           # Target triples, data models, C type layouts
  rcc_session/          # Options, CLI-facing session
  rcc_data_structures/  # FxHashMap, IndexVec, new_index!
  rcc_lexer/            # Character stream -> pp-tokens
  rcc_preprocess/       # Macros, conditional compilation, #include
  rcc_ast/              # Concrete-ish C AST
  rcc_parse/            # pp-tokens -> AST (recursive descent + Pratt)
  rcc_hir/              # Name-resolved tree + Ty/TyCtxt
  rcc_hir_lower/        # AST -> HIR
  rcc_typeck/           # C99 conversions, const-eval
  rcc_cfg/              # MIR-style CFG (BasicBlock/Terminator/Body)
  rcc_cfg_transform/    # CFG passes
  rcc_codegen_llvm/     # CFG -> LLVM IR (inkwell, behind `llvm` feature)
  rcc_driver/           # `rcc` binary: CLI + pipeline orchestration
  rcc_conformance/      # External test-suite runner + reports
xtask/                 # fetch, coverage, fuzz, benchmark, release helpers
third_party/
  MANIFEST.toml        # Pinned external C test suites
  testsuites/          # Populated by `cargo xtask fetch-testsuites`
fuzz/                  # cargo-fuzz targets (extended/manual)
docs/
  architecture.md
  interfaces.md
  testing.md
  conformance.md
```

## Install

`rcc` is published on crates.io as **`rcc-compiler`** because the crate name
`rcc` is already taken. The installed executable is still named `rcc`.

The current crates.io release is `0.0.0`. This is an early distribution
release, not a 1.0 stability release; fuzzing and conformance work remain
active.

The supported install path is Linux / WSL with LLVM 18 available:

```bash
# Example for Debian/Ubuntu/WSL after installing LLVM 18 from apt.llvm.org.
export LLVM_SYS_181_PREFIX=/usr/lib/llvm-18
cargo install rcc-compiler --version 0.0.0
rcc --version --verbose
```

Smoke test an installed compiler:

```bash
cat > /tmp/rcc-hello.c <<'EOF'
int puts(const char *);
int main(void) { puts("hello from rcc"); return 0; }
EOF

rcc /tmp/rcc-hello.c -O2 -o /tmp/rcc-hello
/tmp/rcc-hello
```

Windows is currently supported as a development host for selected LLVM-C tests,
but the release install target is still `x86_64-unknown-linux-gnu`; use WSL for
the published `cargo install` path.

## Build

The default configuration builds **without** LLVM so that contributors
can hack on the front-end on any machine.

```bash
cargo build --workspace
cargo test  --workspace
```

To enable the LLVM code generator install LLVM 18 (with `llvm-config`
on `PATH`) and build with the feature flag:

```bash
# Linux / macOS: see https://apt.llvm.org/ or `brew install llvm@18`
export LLVM_SYS_181_PREFIX=/usr/lib/llvm-18
cargo build --features rcc_codegen_llvm/llvm
```

On Windows hosts with the official `clang+llvm-18.1.8-x86_64-pc-windows-msvc`
archive, use the project-supported LLVM-C import library path. This is
Windows host support, not Windows target support:

```powershell
$env:LLVM_SYS_181_PREFIX='D:\Tools\clang+llvm-18.1.8-x86_64-pc-windows-msvc'
$env:Path="$env:LLVM_SYS_181_PREFIX\bin;$env:Path"
cargo test -p rcc_codegen_llvm --features llvm-windows-llvm-c --test llvm_ir_snapshots -- --test-threads=1
```

## Running the compiler

```bash
cargo run --bin rcc -- path/to/file.c --emit=ast
cargo run --bin rcc -- path/to/file.c --emit=llvm-ir -o out.ll
```

Supported `--emit` stages: `tokens`, `pp`, `ast`, `hir`, `mir`,
`llvm-ir`, `asm`, `obj`.

## External test suites

Test suites are pinned in [`third_party/MANIFEST.toml`](third_party/MANIFEST.toml)
and fetched on demand:

```bash
# Permissive-licensed suites (c-testsuite, chibicc, llvm-test-suite, csmith)
cargo xtask fetch-testsuites

# Including GPL-licensed suites (gcc-torture, tcc-tests2) — they run in a
# separate process; their sources are NOT linked into any rcc binary.
cargo xtask fetch-testsuites --include-gpl
```

See [`docs/conformance.md`](docs/conformance.md) for the running
pass-rate dashboard and [`LICENSES/README.md`](LICENSES/README.md) for
license bookkeeping.

## Documentation

- [`docs/architecture.md`](docs/architecture.md) — pipeline, crate roles,
  key invariants.
- [`docs/interfaces.md`](docs/interfaces.md) — public type signatures at
  every crate boundary.
- [`docs/testing.md`](docs/testing.md) — unit / boundary / integration /
  fuzz test strategy.
- [`docs/conformance.md`](docs/conformance.md) — external test-suite
  progress.
- [`docs/ci.md`](docs/ci.md) — mandatory vs exploratory GitHub Actions.
- [`docs/platform-support.md`](docs/platform-support.md) — supported hosts,
  release target, LLVM/tool discovery, and libc/linker boundaries.
- [`docs/release-checklist.md`](docs/release-checklist.md) — release gates in
  execution order.

The original architecture plan under `.cursor/plans/` is historical and
read-only for agents. The executable source of truth for remaining work is the
task tree under [`tasks/`](tasks/).

## License

The rcc compiler is dual-licensed **MIT OR Apache-2.0**. Vendored test
suites keep their upstream licenses; see [`LICENSES/`](LICENSES/).
