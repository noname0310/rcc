# 09-24: Windows LLVM-C linking feature

**Phase:** 09-codegen-llvm    **Depends on:** 09-23    **Milestone:** M4    **Size:** Medium

## Goal

Make the LLVM-enabled codegen tests build on Windows with the official
`clang+llvm-18.1.8-x86_64-pc-windows-msvc` archive without requiring ad hoc
`RUSTFLAGS` in every shell.

## Problem

The official Windows archive contains `LLVM-C.dll` and `LLVM-C.lib`, but not
`libxml2s.lib` or `LLVM-18.dll`. The default `llvm-sys`/Inkwell static path asks
`llvm-config --system-libs --link-static` for system libraries and then fails on
the missing `libxml2s.lib`. The normal dynamic path asks `llvm-config
--link-shared`, but the archive does not provide the generic `LLVM-18.dll`.

The locally verified workaround is:

```powershell
$llvm = 'D:\Tools\clang+llvm-18.1.8-x86_64-pc-windows-msvc'
$env:LLVM_SYS_181_PREFIX = $llvm
$env:Path = "$llvm\bin;$env:Path"
$env:RUSTFLAGS = "-L native=$llvm\lib -l dylib=LLVM-C"
cargo test -p rcc_codegen_llvm --features "llvm inkwell/llvm18-1-no-llvm-linking" --test llvm_ir_snapshots
```

This task turns that workaround into a project-supported feature.

## Scope

- In: Cargo feature wiring, `build.rs` link directives, Windows-only diagnostics,
  and a documented local command.
- Out: custom static LLVM builds, vendoring LLVM, or changing Linux CI's
  existing `rcc_codegen_llvm/llvm` feature path.

## Deliverables

- A feature such as `llvm-windows-llvm-c` that enables `llvm` plus
  `inkwell/llvm18-1-no-llvm-linking` on Windows.
- `crates/rcc_codegen_llvm/build.rs` logic, gated by the new feature, that:
  - reads `LLVM_SYS_181_PREFIX`,
  - verifies `<prefix>\lib\LLVM-C.lib` exists,
  - emits `cargo:rustc-link-search=native=<prefix>\lib`,
  - emits `cargo:rustc-link-lib=dylib=LLVM-C`,
  - prints a clear error with the expected LLVM 18 archive layout when the
    environment is wrong.
- Documentation for the Windows command and required `Path` update so
  `LLVM-C.dll` is found at test/runtime.
- A smoke test or CI-safe build-script unit that validates path construction
  without requiring LLVM in non-LLVM jobs.

## Acceptance

- On the local Windows machine with:

  ```powershell
  $env:LLVM_SYS_181_PREFIX='D:\Tools\clang+llvm-18.1.8-x86_64-pc-windows-msvc'
  $env:Path="$env:LLVM_SYS_181_PREFIX\bin;$env:Path"
  ```

  this passes without manual `RUSTFLAGS`:

  ```powershell
  cargo test -p rcc_codegen_llvm --features llvm-windows-llvm-c --test llvm_ir_snapshots -- --test-threads=1
  ```

- The existing Linux/CI LLVM command using `rcc_codegen_llvm/llvm` still works.
- If `LLVM_SYS_181_PREFIX` is missing or points to a directory without
  `LLVM-C.lib`, the build fails with an actionable message.
- No project-wide default feature starts requiring LLVM.

## References

- LLVM `llvm-config` options: `--system-libs`, `--link-static`, `--link-shared`
- LLVM 18 CMake docs: `LLVM_BUILD_LLVM_DYLIB` is not available on Windows
- Inkwell `llvm18-1-no-llvm-linking` feature
- LLVM bug 172890: Windows archive reports `libxml2s.lib` through
  `llvm-config` but does not ship it
