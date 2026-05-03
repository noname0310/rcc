# 10-15: Exit status and diagnostics contract

**Phase:** 10-driver    **Depends on:** 10-00.2, 10-01, 10-02, 10-03    **Milestone:** M4    **Size:** Small

## Goal

Define and enforce when `rcc` returns success, compilation failure, CLI usage
failure, or infrastructure failure. Conformance runners and CI should be able
to trust exit codes.

## Scope

- In:
  - Stable exit-code enum in `rcc_driver`.
  - Parse/typeck/CFG/codegen diagnostics return a compilation-failure code.
  - CLI misuse returns a usage-failure code.
  - I/O, missing linker, missing LLVM, and subprocess failures return an
    infrastructure-failure code.
  - No link step runs after front-end diagnostics.
- Out:
  - Rich diagnostic JSON format.

## Deliverables

- `ExitCode` / `DriverStatus` with documented numeric values.
- Integration tests for parse error, type error, missing input file, missing
  linker/tool, and successful compile.
- Conformance adapter update if it currently treats all non-zero status values
  identically.

## Acceptance

- `rcc bad.c` returns compilation failure, not success.
- `rcc --unknown` returns usage failure.
- Backend-disabled, missing linker, and failed subprocess cases return
  infrastructure failure, not success.
- `rcc hello.c -o out` does not invoke the linker if type checking emitted an
  error.
- Test harnesses can distinguish compiler bugs from expected invalid-source
  diagnostics.

## References

- POSIX `sysexits.h` categories for inspiration
- rustc `interface::Result` / driver exit behavior
