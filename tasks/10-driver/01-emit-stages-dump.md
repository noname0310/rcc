> ✓ done — 2026-05-04

# 10-01: `--emit=` stage dumps

**Phase:** 10-driver    **Depends on:** 03-13, 05-28, 08-14, 09-15, 10-00.2    **Milestone:** M3

## Goal
Connect every `EmitKind` to a pretty-printer already implemented in
the relevant crate. When multiple emit kinds are requested, write each
to `<output>.<stage>` (or stdout when single + no output).

## Scope
- In: pipeline branch per stage; `pretty` submodules on lexer
  (`rcc_lexer::pretty`, task 03-13) and cfg (`rcc_cfg::pretty`,
  task 08-14) already exist. **AST pretty-printer
  (`rcc_ast::pretty`) does NOT have a prior task — implement it
  here** (or as a preceding sub-task) so `--emit=ast` can work.
  HIR pretty-printer also added here if missing. `--emit=llvm-ir`
  must print / write `CodegenArtifact::ir_text` instead of discarding it.
- Out: machine-readable JSON dumps (future task).

## Deliverables
- `pipeline.rs` dispatching to pretty-printers.
- AST and HIR dumps that are real deterministic dumps, not stderr summary
  placeholders.
- LLVM IR dump path wired to the backend artifact.
- Smoke test for each `EmitKind` value.

## Acceptance
- `rcc hello.c --emit=tokens --emit=ast -o out` produces `out.tokens`
  and `out.ast`.
- `rcc hello.c --emit=hir --emit=mir --emit=llvm-ir -o out` produces
  deterministic `out.hir`, `out.mir`, and `out.ll` / `out.llvm-ir`
  stage artifacts when the LLVM backend is enabled.
- In a no-LLVM build, `--emit=llvm-ir` fails with the backend-required
  contract from `10-00.2`; frontend-only stages still work.
- `--emit=ast` no longer writes the placeholder `"-- emit=ast: N decls"` to
  stderr as its primary output.

## References
- Skeleton driver's existing stubs.
