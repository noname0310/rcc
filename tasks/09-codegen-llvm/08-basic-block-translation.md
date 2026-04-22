# 09-08: Basic-block translation

**Phase:** 09-codegen-llvm    **Depends on:** 09-06    **Milestone:** M3

## Goal
1:1 map `rcc_cfg::BasicBlockId` → `inkwell::BasicBlock`. Statements
in order; terminator as the block's final instruction.

## Scope
- In: a `bb_map: IndexVec<BasicBlockId, BasicBlock>` built up front
  (so forward refs to later blocks exist at translation time);
  `TerminatorKind` dispatch to branch / switch / return / call.
- Out: --.

## Deliverables
- Block translator with explicit match arms per terminator.

## Acceptance
- No "block already terminated" errors during codegen.
- LLVM verifier accepts the emitted function.

## References
- rustc `rustc_codegen_llvm` block wiring.
