> ✓ done — 2026-05-06

# 30: Constant If Dead Label Continuation

**Phase:** 08-cfg    **Depends on:** 08-29    **Milestone:** real-world/lua follow-up

## Goal

Keep C labels inside constant-folded dead `if` branches valid CFG targets.

## Scope

In:

- `if (0) { label:; }` where a later `goto label;` reaches the label.
- Dead branch label lowering that falls through to the post-`if` continuation.
- A reduced CFG regression for the `vla_backward_goto_dealloc` e2e panic.

Out:

- Changing C legality checks for jumping into VLA scopes.
- Reworking general dead-code label lowering for every compound statement shape.

## Acceptance

- [x] A constant-false branch with a target label no longer leaves a reachable
  un-terminated label block.
- [x] The reduced CFG test verifies the label block has a real continuation
  terminator instead of default `Unreachable`.
- [x] `cargo test -p rcc_cfg --test cfg` passes.
- [x] `cargo test -p rcc_driver --test e2e --features llvm` passes on WSL with LLVM 18.

## Notes

The failure was exposed by `crates/rcc_driver/tests/e2e/vla_backward_goto_dealloc.c`.
Constant-condition pruning correctly avoided lowering dead calls, but it also
discarded labels that remain reachable through `goto`. When the later `goto`
targeted the pre-collected label block, debug CFG finalization panicked because
the reachable label block still had no terminator. Release builds surfaced the
same shape as a CFG verifier `ReachableUnreachableTerminator`.
