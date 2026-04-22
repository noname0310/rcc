# 09-06: Function body emission

**Phase:** 09-codegen-llvm    **Depends on:** 09-01 .. 09-05    **Milestone:** M3

## Goal
Translate a `rcc_cfg::Body` into an `inkwell::FunctionValue`. Iterate
blocks in numeric order; emit allocas in the entry (task 07);
translate statements + terminators into LLVM instructions.

## Scope
- In: `CodegenCx::codegen_body(body)`; collaborates with tasks
  08/09/10 for the actual translation.
- Out: whole-module orchestration (driver tasks).

## Deliverables
- `FnBodyCodegen` helper.
- End-to-end smoke: `int main(){ return 0; }` → runnable `./a.out`.

## Acceptance
- The `.ll` output for `return 0;` passes `llvm-as` and runs to
  exit status 0 after `llc` + host ld.

## References
- inkwell `FunctionValue` docs.
