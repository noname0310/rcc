# 10-16: Tool discovery and verbose trace

> ✓ done — 2026-05-04 — implemented in commit

**Phase:** 10-driver    **Depends on:** 10-02, 10-10, 10-14    **Milestone:** M5    **Size:** Medium

## Goal

Centralize LLVM-oriented tool discovery and make `-v` useful. Every subprocess
the driver may invoke should be visible, reproducible, and tested without
actually requiring every tool in no-LLVM jobs.

## Scope

- In:
  - Tool lookup for clang-compatible linker drivers, `lld`, LLVM tools, and
    object dump tools.
  - Environment override variables such as `RCC_LINKER_DRIVER`, `RCC_CLANG`,
    `RCC_LLD`, `RCC_LLVM_PREFIX`, or the existing LLVM prefix env.
  - Default final linking through `clang -fuse-ld=lld`, not host `cc`.
  - `-v` output: version, target, include paths, selected tools, and exact
    subprocess command lines.
  - Dry-run helper for tests so command construction can be asserted without
    executing external tools.
- Out:
  - Package manager installation guidance.
  - Remote toolchains.

## Deliverables

- `Toolchain` / `ToolFinder` module in `rcc_driver`.
- `CommandSpec` value type separate from `std::process::Command`.
- `-v` CLI parsing and tests.
- Tool-not-found diagnostics with actionable search paths.

## Acceptance

- `rcc -v hello.c` prints version, target, selected linker, and each command to
  stderr.
- Tests can assert linker command arguments without spawning clang/lld.
- Missing tools produce deterministic diagnostics and infrastructure-failure
  exit codes.

## References

- rustc linker flavor / command construction split
- Clang `-###` and `-v` output behavior
