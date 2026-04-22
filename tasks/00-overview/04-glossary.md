# 00-04: Glossary

**Phase:** 00-overview    **Depends on:** —    **Milestone:** —

| Term | Meaning |
|------|---------|
| **pp-token** | Preprocessing token per C99 §6.4. Output of `rcc_lexer`, input to `rcc_preprocess`. |
| **token**    | Full C token per C99 §6.4 (keywords resolved, literals decoded). Output of `rcc_parse` phase-7 conversion. |
| **AST**      | `rcc_ast::TranslationUnit` — concrete-ish tree, no name resolution. |
| **HIR**      | `rcc_hir::HirCrate` — name-resolved, typed tree. |
| **CFG / MIR** | `rcc_cfg::Body` — basic blocks + terminators, non-SSA (alloca + load/store). |
| **`Ty` / `TyCtxt`** | Interned C type representation, owned by `rcc_hir::TyCtxt`. |
| **hide set** | Prosser macro-expansion algorithm's per-token set of macros that must not re-expand. Lives in `rcc_preprocess::macros::HideSet`. |
| **typedef-name hack** | Parser disambiguation: lookup the current scope to decide if an identifier is a `typedef-name`. |
| **UAC**      | Usual Arithmetic Conversion (C99 §6.3.1.8). Implemented by `rcc_typeck::usual_arithmetic`. |
| **decay**    | Array-to-pointer / function-to-pointer conversion (C99 §6.3.2.1). |
| **lvalue / rvalue** | Value category per C99 §6.3.2.1; tracked in `rcc_hir::ValueCat`. |
| **xfail**    | Expected failure: listed in a suite's `xfail.toml`; counts as pass for CI gating but tracked separately. |
| **KPI cell** | Single (milestone, suite) pair in the matrix from [`02-kpi-dashboard.md`](02-kpi-dashboard.md). |
| **differential** | Compile the same program with `rcc` *and* the host `cc`; compare behaviour. Sole csmith usage. |
| **mem2reg**  | LLVM pass that promotes `alloca` slots to SSA values. Our CFG is designed to rely on it. |
