# 11-16f: tcc-tests2 dead-code CFG panic

> ✓ done — 2026-05-04

**Phase:** 11-conformance    **Depends on:** 11-16    **Milestone:** M6

## Goal
Remove the CFG builder panic when unreachable or dead-code constructs
attempt to terminate an already terminated block.

## Scope
- In: `tcc-tests2::87_dead_code` and `tcc-tests2::89_nocode_wanted`.
- Out: output-mismatch fixes after the panic is gone, unless they are tiny and
  directly caused by the same CFG bug.

## Deliverables
- A minimal CFG builder regression test for the double-termination panic.
- A builder invariant change that makes unreachable continuation blocks
  explicit instead of panicking.

## Acceptance
- Neither target case exits with Rust panic/exit 101.
- `BodyBuilder::terminate: block ... is already terminated` is covered by a
  regression test.

## Result
- Fixed conditional and short-circuit lowering so an arm that already
  terminates does not receive an extra `goto` to the join block.
- Fixed GNU statement-expression lowering to create an explicit unreachable
  cleanup block when the statement-expression body terminates before scope
  exit, keeping StorageLive/StorageDead structurally balanced.
- `tcc-tests2::87_dead_code` and `tcc-tests2::89_nocode_wanted` both pass.
- WSL tcc-tests2 baseline after this task: 88 discovered, 70 pass, 9 xfail,
  5 fail, 4 skip.

## References
- `target/wsl/tcc-tests2-16-final.json`
- `target/wsl/tcc-tests2-16f-final.json`
- `crates/rcc_cfg/src/build.rs`.
