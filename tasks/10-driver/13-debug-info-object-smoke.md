> ✓ done — 2026-05-04 — implemented in commit

# 10-13: Debug info object smoke

**Phase:** 10-driver    **Depends on:** 09-26, 10-02, 10-07    **Milestone:** M6    **Size:** Medium

## Goal

Complete the object-level half of DWARF support that was intentionally split
out of `09-codegen-llvm/26-debug-ir-metadata.md`.

## Scope

- In:
  - `-g` CLI parsing and `Options::debug_info` wiring.
  - Object-file generation path with debug metadata enabled.
  - Tool-gated `llvm-dwarfdump` or `llvm-readobj --sections` smoke tests.
  - Negative smoke proving non-`-g` object output has no debug section.
- Out:
  - Optimized debug info quality.
  - Cross-platform debugger integration.
  - Split DWARF and DWARF v5 feature work.

## Deliverables

- `rcc -g -c hello.c -o hello.o` driver path.
- Test helper that discovers `llvm-dwarfdump` / `llvm-readobj` from the active
  LLVM prefix or `PATH`, and skips with a clear reason when unavailable.
- Fixture checking function name, parameter name, local name, and line record
  presence in dumped debug info.

## Acceptance

- `rcc -g -c hello.c -o hello.o` produces an object containing `.debug_info`
  or the COFF equivalent reported by LLVM tools.
- `rcc -c hello.c -o hello.o` produces an object without debug sections.
- The smoke test is skipped, not failed, when LLVM object inspection tools are
  unavailable in a no-LLVM environment.

## References

- `tasks/09-codegen-llvm/26-debug-ir-metadata.md`
- LLVM `llvm-dwarfdump`
- LLVM `llvm-readobj --sections`
