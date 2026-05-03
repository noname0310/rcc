# 10-00.2: Backend-required output contract

**Phase:** 10-driver    **Depends on:** 09-26, 10-00.1    **Milestone:** M3    **Size:** Small

## Goal

Stop no-LLVM builds from silently succeeding when the requested driver action
requires the LLVM backend. A frontend-only request such as `--emit=tokens` or
`--emit=pp` may succeed without LLVM; default compilation, `--emit=llvm-ir`,
`--emit=asm`, and `--emit=obj` must not report success if
`rcc_codegen_llvm` returns `BackendDisabled`.

## Scope

- In:
  - Define a driver predicate for "this invocation requires backend output".
  - Convert `CodegenError::BackendDisabled` into a non-zero driver result for
    backend-required invocations.
  - Preserve successful no-LLVM behavior for frontend-only emits (`tokens`,
    `pp`, and parse-only diagnostics where no backend is requested).
  - Add tests in the default no-LLVM workspace configuration.
- Out:
  - Object writing and linker invocation (`10-02`).
  - Stable numeric exit-code taxonomy (`10-15` owns the final enum).
  - LLVM feature build behavior.

## Deliverables

- Driver pipeline change that no longer swallows `BackendDisabled` as success
  when backend output is required.
- Tests covering:
  - default `rcc hello.c` in a no-LLVM build returns failure;
  - `rcc hello.c --emit=llvm-ir` in a no-LLVM build returns failure;
  - `rcc hello.c --emit=tokens` and `--emit=pp` remain successful.

## Acceptance

- A no-LLVM build cannot pass an E2E/link smoke test by doing nothing.
- Backend-disabled diagnostics mention that the LLVM backend is unavailable.
- No output or temp artifact is created for a failed backend-required
  invocation.
- Later `10-02` link tests can rely on "success means the backend produced an
  artifact".

## References

- `crates/rcc_driver/src/pipeline.rs`: current `BackendDisabled => Ok(())`
  behavior.
- `tasks/10-driver/02-link-invocation.md`.
- `tasks/10-driver/15-exit-status-and-diagnostics-contract.md`.
