> ✓ done — 2026-05-05 — implemented in commit

# 10-19: Parallel multi-input object builds

**Phase:** 10-driver    **Depends on:** 10-11, 10-16    **Milestone:** M5

## Goal
Compile independent C translation units concurrently when the driver receives
multiple input files, while preserving `gcc`/`clang`-style quiet output.

## Scope
- In: `rcc -c a.c b.c ...` and `rcc a.c b.c ... -o prog` schedule per-TU
  object compilation across a bounded worker pool. Linking still runs once,
  after all successful temporary objects are available.
- In: expose `-j/--jobs N` and `RCC_JOBS=N` to cap driver parallelism. The
  default is host parallelism capped by the number of input files.
- In: keep `-E`, dependency emission, `--emit=*`, and `-S` multi-input modes
  serial because their stdout/artifact ordering is externally observable.
- In: make LLVM target initialization idempotent under concurrent backend use.
- Out: cargo-style progress UI. `rcc` must remain quiet on success so it can
  substitute for `gcc`/`clang` in existing build scripts.

## Deliverables
- `Cli::jobs` flag and environment fallback.
- Driver worker-pool scheduler for object-producing multi-input modes.
- Stable object/link order independent of worker completion order.
- LLVM target initialization guarded by `Once`.
- Tests for `-j` parsing and parallel compile-only behavior.

## Acceptance
- `rcc -j 2 -c main.c util.c` produces `main.o` and `util.o`.
- `rcc -j 2 main.c util.c -o prog` links objects in command-line order.
- A failing TU does not suppress compilation of other TUs.
- Successful builds do not print cargo-style progress lines.
- `cargo test -p rcc_driver --features llvm multi_file` passes on an LLVM host.

## References
- `10-11: Multi-file compilation` explicitly left parallel compilation as a
  future optimisation.
- rustc/Cargo split: rustc owns diagnostics and codegen, while orchestration UI
  belongs to Cargo. `rcc` is a compiler driver, so this task intentionally keeps
  orchestration silent.
