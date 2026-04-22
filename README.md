# rcc — a Rust-based C99 compiler

`rcc` is a C99 compiler written in Rust, targeting **LLVM IR** through
[`inkwell`](https://github.com/TheDan64/inkwell). Its architecture is
modelled after the `rustc` multi-crate workspace: each stage
(preprocess / lex / parse / HIR / typeck / MIR / codegen) lives in its
own crate and communicates through narrow public type boundaries.

> **Status.** This repository is the *skeleton* produced from the
> architecture plan. Every crate compiles and exposes its public types,
> but the bodies of most passes are deliberately empty — the whole point
> is to freeze interfaces so that feature work (M1–M7 in the plan) can
> proceed in parallel without merge conflicts.

## Repository layout

```
crates/
  rcc_span/             # Spans, SourceMap, Symbol interner
  rcc_errors/           # Diagnostic, Handler, Emitter
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
xtask/                 # cargo xtask fetch-testsuites / show-manifest
third_party/
  MANIFEST.toml        # Pinned external C test suites
  testsuites/          # Populated by `cargo xtask fetch-testsuites`
fuzz/                  # cargo-fuzz targets (nightly)
docs/
  architecture.md
  interfaces.md
  testing.md
  conformance.md
```

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
export LLVM_SYS_180_PREFIX=/usr/lib/llvm-18
cargo build --features rcc_codegen_llvm/llvm
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

The source of truth for high-level design decisions is the plan at
`.cursor/plans/c_compiler_architecture_plan_*.plan.md`.

## License

The rcc compiler is dual-licensed **MIT OR Apache-2.0**. Vendored test
suites keep their upstream licenses; see [`LICENSES/`](LICENSES/).
