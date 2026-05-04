# 11d: function.c ABI runtime smoke

**Phase:** 11-conformance    **Depends on:** 11a, 11b, 11c    **Milestone:** M6

## Goal
Before running the entire `function.c`, isolate and verify the ABI/runtime
surfaces it exercises so failures point at one owner instead of a 350-line TU.

## Scope
- In:
  - Build reduced fixtures for:
    - small integer return narrowing (`char`, `short`, `_Bool`);
    - fixed and variadic integer calls;
    - float and double arguments/returns;
    - many register/stack arguments;
    - small and large struct arguments;
    - small and large struct returns;
    - `long double` cast/compare used by `function.c`.
  - Run them through the LLVM-enabled E2E harness on WSL/Linux.
  - Convert every failure into an owner task or fix it directly if it is local.
- Out:
  - Full chibicc suite green.
  - Non-SysV ABI coverage.

## Deliverables
- One or more reduced E2E fixtures or conformance adapter cases.
- A short matrix in this file listing each slice and its result.

## Acceptance
- Every reduced ABI slice needed by `function.c` passes or has a concrete owner
  task.
- No compiler crash or backend `unsupported ...` error remains in the reduced
  slices.
- `12-chibicc-function-green.md` can focus on the full TU rather than unknown
  prerequisites.

## References
- chibicc `test/function.c`.
- `tasks/09-codegen-llvm/07-sysv-abi-params.md`.
- `tasks/09-codegen-llvm/08-sysv-abi-returns.md`.
- `tasks/09-codegen-llvm/13-call-emission-with-abi.md`.
