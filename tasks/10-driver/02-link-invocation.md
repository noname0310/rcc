> ✓ done — 2026-05-04

# 10-02: Link invocation

**Phase:** 10-driver    **Depends on:** 09-23    **Milestone:** M3

## Goal
Drive final executable production. Emit an object file from the LLVM backend
(`TargetMachine::write_to_memory_buffer` or an equivalent object writer), then
call the host C compiler as the linker so libc / crt startup is handled
correctly.

## Scope
- In:
  - Extend `CodegenArtifact` or add a driver-side backend API that can produce
    object bytes for one translation unit.
  - Write object bytes to the artifact/temp policy used by the driver.
  - Locate host `cc` from `PATH` as a first pass.
  - Build a command passing our `.o` plus `-o <output>`.
  - Surface non-zero linker status with stderr and the command line.
- Out:
  - Custom linker scripts.
  - User-controlled linker selection and extra linker flags (`10-10`,
    `10-16`).
  - Multi-file linking (`10-11`).

## Deliverables
- Object emission API used by the driver.
- `pipeline::link(obj: &Path, output: &Path) -> Result<(), String>` or a
  `CommandSpec`-based equivalent if `10-16` has already landed.
- Fixture: compile `int main(){return 42;}` → run → exit 42.

## Acceptance
- On Linux / macOS / Windows, `rcc hello.c -o hello` and `./hello`
  works.
- A linker failure includes the linker command and stderr in the diagnostic.
- The no-LLVM build still reports backend-disabled rather than trying to link
  an empty/nonexistent object.

## References
- rustc `link.rs` for design inspiration (much simpler here).

## Completion notes
- Windows-native linking is covered by command construction and error-path
  tests in this task. A runnable Windows ABI/COFF E2E is deferred to the target
  abstraction work in `10-08`.
- Runnable E2E was verified under WSL with LLVM 18 and host `cc`, matching the
  backend's current Linux SysV target.
