# Testing strategy

Derived from §8 of the architecture plan. This file is a **checklist for
contributors**: every commit touches the matching bullets.

## Three layers

```
unit tests     ─▶  per-crate cargo test
boundary tests ─▶  adjacent crates wired end-to-end with captured diagnostics
integration    ─▶  driver-level `tests/ui`, snapshot, end-to-end
```

## Per-crate unit tests

| Crate | What to test |
| ----- | ------------ |
| `rcc_span`            | Symbol identity/hash, SourceMap line-col round trip. Covered by `crates/rcc_span/tests/roundtrip.rs`. |
| `rcc_errors`          | DiagnosticBuilder API, `CaptureEmitter` records every level, Handler counters. Covered by `crates/rcc_errors/tests/capture.rs`. |
| `rcc_lexer`           | Table-driven tests per `PpTokenKind`. **Fuzz target `fuzz/fuzz_targets/lex.rs` must run 24 h without panic/hang.** |
| `rcc_preprocess`      | Macro expansion: recursion blocker (hide set), `##`/`#`, variadic `__VA_ARGS__`, self-reference, `#if`/`#elif` const-eval, include search path (mocked), `#pragma once`. |
| `rcc_parse`           | One positive + one negative case per grammar production. Regression test for the typedef-name hack (`typedef int T; T x;` vs `int T; T x;`). Error-recovery tests. |
| `rcc_ast` / `rcc_hir`  | Visitor round-trip, `NodeId`/`HirId` uniqueness. |
| `rcc_hir_lower`       | Declarator-tree → `Ty` table, including `int (*fp[3])(int,int)` and abstract declarators. |
| `rcc_typeck`          | Integer promotion table (`usual_arithmetic` currently in `rcc_typeck/src/lib.rs`). ConstEval edge cases (`INT_MIN / -1`, `1u - 2`, ...). |
| `rcc_cfg`             | Snapshot (`insta`) MIR dumps for `if`, `while`, `switch`, `goto`. |
| `rcc_codegen_llvm`    | `FileCheck`-style `// CHECK:` matching on `CodegenArtifact::ir_text`. |
| `rcc_conformance`     | Report serialisation round trip; `xfail.toml` parser. |

Coverage target per crate: **80 %** (`cargo llvm-cov`, enforced by CI).

## Boundary tests

| Contract | Assertion |
| -------- | --------- |
| `rcc_lexer` ↔ `rcc_preprocess`     | Every `PpToken` pp-token kind appears at most in contexts legal for a preprocessor input. |
| `rcc_preprocess` ↔ `rcc_parse`     | Expanded stream contains no `#`/`##`, no `Newline` except directive boundaries. |
| `rcc_parse` ↔ `rcc_hir_lower`      | Every `NodeId` in the AST is referenced exactly once by the resulting HIR. |
| `rcc_typeck` ↔ `rcc_cfg`           | Every `HirExpr` has `ty != TyCtxt::error` or a diagnostic was emitted. |
| `rcc_cfg` ↔ `rcc_codegen_llvm`     | Every `BasicBlock` has a `Terminator`. Every `Place` references an existing `Local`. |

Each contract lives in a dedicated `tests/<contract>.rs` inside the
**downstream** crate, which pulls in the upstream as a normal dependency.

## Integration tests

Driver-level tests live in `crates/rcc_driver/tests/`:

- **UI tests** (`tests/ui/**/*.c` + `.stderr` goldens). Run with
  `--emit=checked`; compare stderr byte-for-byte. Equivalent to
  `rustc`'s `tests/ui`.
- **Snapshot tests** (`insta`): one test per `--emit` stage (`tokens`,
  `pp`, `ast`, `hir`, `mir`, `llvm-ir`).
- **End-to-end** (`tests/e2e/*.c`): compile → `llc` + system linker →
  run → compare stdout / exit code. Cross-compile the same file with
  `cc` and compare **execution** — this is the differential oracle for
  implementation correctness.

## Fuzzing & differential testing

Targets live in [`fuzz/fuzz_targets/`](../fuzz/fuzz_targets/):

- `lex.rs` — bytes → `rcc_lexer::tokenize` should never panic or spin.
- `preprocess.rs` — bytes → `rcc_preprocess::preprocess` should likewise
  converge.

Differential testing with `csmith` is driven by
`rcc_conformance::adapters::CsmithDifferentialAdapter`: generate a
program, compile+run with both `rcc` and `cc`, compare stdout + exit
code.

CI budgets:
- per-commit: 30 s fuzz smoke on `lex` (see `.github/workflows/ci.yml`).
- nightly: 24 h fuzz + 1 h csmith differential (configured out of band).

## CI gates (see `.github/workflows/ci.yml`)

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace` (no-LLVM path, fast)
4. `cargo test --workspace --features rcc_codegen_llvm/llvm` (LLVM path)
5. `cargo llvm-cov --workspace` — coverage uploaded; threshold enforced.
6. 30-second `cargo fuzz run lex` smoke.
7. `cargo xtask fetch-testsuites` + conformance run against
   **c-testsuite** + the milestone-appropriate **chibicc** subset.

Each milestone (M1..M7 in the plan) tightens gate 7 with a concrete
pass-rate target (see [`conformance.md`](conformance.md)).
