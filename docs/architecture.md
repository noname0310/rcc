# rcc architecture

This document is the code-facing version of the high-level plan at
`.cursor/plans/c_compiler_architecture_plan_*.plan.md`. The plan owns the
*why*; this file owns the *what is actually in the tree*.

## Pipeline

```
    .c source
      │
      ▼
 ┌────────────┐  chars       ┌────────────┐  pp-tokens   ┌────────────┐
 │ SourceMap  │ ───────────▶ │ rcc_lexer   │ ───────────▶ │rcc_preprocess│
 └────────────┘              └────────────┘              └────────────┘
                                                               │
                                                               ▼
                              ┌────────────┐    AST    ┌────────────┐
                              │ rcc_parse   │ ────────▶ │ rcc_ast      │
                              └────────────┘           └────────────┘
                                                               │
                                                               ▼
                                                       ┌────────────┐
                                                       │rcc_hir_lower│
                                                       └────────────┘
                                                               │ HIR
                                                               ▼
                              ┌────────────┐   typed   ┌────────────┐
                              │ rcc_typeck  │ ◀───────▶ │  rcc_hir     │
                              └────────────┘           └────────────┘
                                                               │
                                                               ▼
                              ┌────────────┐    Body   ┌────────────┐
                              │ rcc_cfg     │ ────────▶ │rcc_cfg_trans│
                              └────────────┘           └────────────┘
                                                               │
                                                               ▼
                                                      ┌────────────────┐
                                                      │rcc_codegen_llvm │
                                                      └────────────────┘
                                                               │
                                                               ▼
                                                      LLVM IR → .o / .ll
```

## Crate roles (matches `rustc` layout)

| rcc crate              | rustc analogue             | Responsibility |
| --------------------- | -------------------------- | -------------- |
| `rcc_span`             | `rustc_span`               | `Span`, `SourceMap`, `Symbol` interner |
| `rcc_errors`           | `rustc_errors`             | `Diagnostic`, `Handler`, `Emitter` |
| `rcc_session`          | `rustc_session`            | `Options`, session-wide state |
| `rcc_data_structures`  | `rustc_data_structures`    | `FxHashMap`, `IndexVec`, `new_index!` |
| `rcc_lexer`            | `rustc_lexer`              | Char stream → pp-tokens |
| `rcc_preprocess`       | — (C-specific)             | Macros, `#include`, conditionals |
| `rcc_ast`              | `rustc_ast`                | Concrete-ish AST + visitor |
| `rcc_parse`            | `rustc_parse`              | pp-tokens → AST, typedef disambiguation |
| `rcc_hir`              | `rustc_hir`                | Name-resolved tree + C99 `Ty`/`TyCtxt` |
| `rcc_hir_lower`        | `rustc_ast_lowering`       | AST → HIR, declarator flattening |
| `rcc_typeck`           | `rustc_hir_typeck`         | C99 §6.3 conversions, const-eval |
| `rcc_cfg`              | `rustc_middle::mir` + build| CFG/MIR + HIR → CFG |
| `rcc_cfg_transform`    | `rustc_mir_transform`      | CFG passes |
| `rcc_codegen_llvm`     | `rustc_codegen_llvm`       | CFG → LLVM IR via `inkwell` |
| `rcc_driver`           | `rustc_driver`             | CLI + pipeline orchestration |
| `rcc_conformance`      | — (tests/ harness)         | External test-suite scoring |
| `xtask`               | — (project tooling)        | Vendoring + maintenance tasks |

## Key invariants

1. **Each stage owns one data type.** A crate *produces* one representation
   and *consumes* the predecessor's. AST belongs to `rcc_ast`, HIR to `rcc_hir`,
   CFG/`Body` to `rcc_cfg`.
2. **`Span` everywhere.** Every token, AST node, HIR node, MIR statement,
   and diagnostic carries a `Span`. No synthetic constructs use `DUMMY_SP`
   unless genuinely compiler-generated.
3. **SSA is LLVM's job.** The CFG emits `alloca + load/store`. LLVM
   `mem2reg` handles SSA promotion, so every mutable local is a slot.
4. **Errors are delivered via `Handler`.** Stages never `panic!` on user
   errors; they build a `Diagnostic` and continue with best-effort
   recovery (sentinel `Ty::Error`, skipped nodes, etc.).
5. **LLVM dependency is optional.** `rcc_codegen_llvm` hides `inkwell`
   behind the `llvm` feature so the front-end builds on any machine.

## Translation-phase mapping (C99 §5.1.1.2)

| C99 phase | rcc location |
| --------- | ----------- |
| 1 (trigraph, char-set) | Not implemented (C99 trigraphs are deprecated). |
| 2 (line splicing)      | `rcc_lexer::Cursor` (when fully implemented). |
| 3 (pp-tokenisation + comments) | `rcc_lexer::tokenize`. |
| 4 (directives + macro expansion) | `rcc_preprocess::Preprocessor::run`. |
| 5 (source-charset → execution-charset) | `rcc_parse::token::*Literal`. |
| 6 (adjacent string-literal concat) | `rcc_parse` (phase-7 conversion). |
| 7 (pp-tokens → tokens; parsing; typechecking) | `rcc_parse` + `rcc_hir_lower` + `rcc_typeck`. |
| 8 (linking) | Delegated to system linker via `rcc_codegen_llvm` + driver. |

## Further reading

- [`interfaces.md`](interfaces.md): the frozen public type signatures.
- [`testing.md`](testing.md): what lives in every crate's `tests/` dir
  and which suites CI demands green.
- [`conformance.md`](conformance.md): numeric progress against
  vendored test suites.
