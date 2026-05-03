# 10-05: End-to-end runner

**Phase:** 10-driver    **Depends on:** 10-02    **Milestone:** M3

## Goal
For each `crates/rcc_driver/tests/e2e/*.c` fixture: build with `rcc`,
link, execute, capture stdout + exit code, compare against expected.

## Scope
- In: test runner invoking subprocess; per-test timeout (10 s);
  emits a readable diff on mismatch.
- Out: differential vs host cc (task 06).

## Deliverables
- Harness + ≥ 10 fixtures (arithmetic, control flow, strings, I/O).

## Acceptance
- `cargo test -p rcc_driver --test e2e --features rcc_codegen_llvm/llvm`:
  green (requires LLVM).

## References
- Plan §8.4.
