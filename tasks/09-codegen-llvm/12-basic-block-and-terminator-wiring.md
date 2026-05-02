> ✓ done — 2026-05-02

# 09-12: Basic block and terminator wiring

**Phase:** 09-codegen-llvm    **Depends on:** 09-06, 09-09, 09-10    **Milestone:** M3

## Goal

Translate CFG block shape into LLVM block shape: one LLVM basic block per
`BasicBlockId`, statements in order, and exactly one terminator per block.

## Scope

- In: pre-create block map, `Goto`, `SwitchInt`, `Return`, `Unreachable`, and
  statement dispatch hooks.
- Out: actual call terminator lowering; owned by 09-13.

## Deliverables

- `FnCodegen::codegen_body` and block translator.
- Verifier tests for branch targets and no double terminators.

## Acceptance

- `if`, `while`, `for`, `break`, `continue`, and simple return fixtures verify
  as LLVM modules.
- Missing CFG terminators become errors instead of malformed LLVM blocks.

## References

- `rcc_cfg::BasicBlock`
- `docs/cfg-semantics.md`
