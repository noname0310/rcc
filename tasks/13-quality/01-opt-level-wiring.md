# 13-01: Optimization level wiring

**Phase:** 13-quality    **Depends on:** 09-14    **Milestone:** M7

## Goal
Make `-O0`, `-O1`, `-O2`, and `-O3` observable all the way from the
driver CLI to LLVM module/codegen output. The current backend already maps
`rcc_session::OptLevel` into LLVM optimization levels; this task hardens the
contract with driver snapshots, IR/ASM differences, and runtime smoke tests.

## Scope
- In:
  - Verify CLI parsing (`-O`, `-O0`, `-O1`, `-O2`, `-O3`) maps to
    `Session.opts.opt_level`.
  - Verify LLVM emission uses that opt level for target machine/pass
    execution.
  - Add snapshot tests where O0 keeps allocas/loads and O2 removes or
    simplifies them for a tiny loop/function fixture.
  - Add one E2E runtime smoke that compiles the same program with `-O0`
    and `-O2` and confirms identical stdout/exit status.
- Out:
  - PGO.
  - LTO; that becomes a separate task if needed because it changes linker
    behavior and release packaging.

## Deliverables
- Driver tests for every supported spelling.
- LLVM IR or ASM snapshot diff for O0 vs O2.
- A short `docs/optimization.md` note explaining which flags are supported.

## Acceptance
- `cargo test -p rcc_session -p rcc_driver --all-targets` passes.
- `cargo test -p rcc_codegen_llvm --features llvm --test llvm_ir_snapshots`
  passes on an LLVM-enabled host.
- `rcc -O2` never silently falls back to O0; tests fail if the opt level is
  ignored.

## References
- LLVM target machine optimization levels.
- Existing `rcc_codegen_llvm::imp::llvm_opt_level`.
