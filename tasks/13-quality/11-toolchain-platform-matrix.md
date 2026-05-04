# 13-11: Toolchain and platform support matrix

**Phase:** 13-quality    **Depends on:** 10-16, 13-10    **Milestone:** M7

## Goal
State exactly which hosts, targets, LLVM versions, and external tools are
supported. This prevents release notes from implying Windows target support
just because a Windows host can find LLVM.

## Scope
- In:
  - Document supported host triples and target triples.
  - Document LLVM 18 discovery on Linux/WSL and Windows hosts.
  - Document linker strategy: `rcc` relies on system/LLVM tools and hosted
    libc; it does not implement libc or a native linker.
  - Add smoke commands for `--print-search-dirs`, `--version --verbose`,
    `--emit=llvm-ir`, object emission, and link+run on supported hosts.
- Out:
  - Implementing Windows target codegen if it is not already supported.
  - Implementing libc/glibc/MSVCRT.

## Deliverables
- `docs/platform-support.md`.
- Driver tests or smoke script for tool discovery output.
- Release-note wording for unsupported targets.

## Acceptance
- Local WSL/Linux hello-world compile+link+run command is documented and
  tested.
- Windows host LLVM C linking setup is documented separately from Windows
  target support.
- Missing external tools produce actionable diagnostics.

## References
- `crates/rcc_driver/src/toolchain.rs`.
- `crates/rcc_codegen_llvm/build_support.rs`.
