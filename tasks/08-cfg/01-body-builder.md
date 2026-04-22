# 08-01: `BodyBuilder`

**Phase:** 08-cfg    **Depends on:** —    **Milestone:** M3

## Goal
Introduce a `BodyBuilder` type that mediates block creation,
statement emission, and terminator fixup. Every lowering fn in this
phase takes `&mut BodyBuilder` and pushes at the current block.

## Scope
- In: `new_block()`, `switch_to(bb)`, `push(stmt)`, `terminate(Terminator)`,
  `finish() -> Body`; post-condition assertions: every reachable
  block has a terminator.
- Out: SSA promotion (LLVM's job).

## Deliverables
- `crates/rcc_cfg/src/build.rs` fleshed out.
- Unit test: build a trivial `return 0;` body and round-trip.

## Acceptance
- `BodyBuilder::finish()` panics (debug) / emits diagnostic (release)
  on an unreachable terminator.

## References
- rustc's `MirBuilder`.
