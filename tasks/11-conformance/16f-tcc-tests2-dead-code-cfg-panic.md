# 11-16f: tcc-tests2 dead-code CFG panic

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

## References
- `target/wsl/tcc-tests2-16-final.json`
- `crates/rcc_cfg/src/build.rs`.
