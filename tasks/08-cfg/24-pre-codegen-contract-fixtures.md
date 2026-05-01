# 08-24: Pre-codegen contract fixtures

**Phase:** 08-cfg    **Depends on:** 08-23, 07-20    **Milestone:** M3 pre-codegen stabilization

## Goal
Add source-level fixtures that lock the final HIR/typeck/CFG contract
before phase 09 starts emitting LLVM IR. These fixtures should catch the
exact failure modes that would otherwise appear as backend bugs.

## Scope
- In: source-to-CFG tests for member access, return coercion, invalid
  pointer coercion, global initializer const-eval, const assignment, and
  volatile access metadata.
- In: tests should go through the real driver pipeline or the same
  preprocess -> parse -> lower -> typeck -> CFG helper used by existing
  edge fixtures.
- In: negative fixtures assert diagnostics and confirm CFG/codegen is
  not invoked.
- Out: LLVM IR snapshots; owned by phase 09.

## Deliverables
- Extend `crates/rcc_cfg/tests/cfg.rs` or add a focused
  `pre_codegen_contract.rs` test file.
- Snapshot or structural assertions for positive fixtures.
- Diagnostic assertions for negative fixtures.

## Acceptance
- `return s.b;` from a second struct field produces CFG with
  `Projection::Field(1)`.
- Invalid return / assignment / call coercions stop before CFG.
- `static int x = 2 + 3;` is represented as a folded global
  initializer.
- `volatile int x;` survives to the pre-codegen contract metadata.

## References
- 06-25, 06-26.
- 07-13 through 07-20.
- 08-23.
