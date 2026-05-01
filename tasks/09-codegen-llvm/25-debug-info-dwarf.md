# 09-25: DWARF debug info

**Phase:** 09-codegen-llvm    **Depends on:** 09-05, 09-12, 09-23    **Milestone:** M6    **Size:** Large

## Goal

Emit DWARF debug metadata through LLVM DIBuilder after the core backend has a
stable type, local, and instruction-location model.

## Scope

- In: `DICompileUnit`, `DIFile`, `DISubprogram`, `DILocalVariable`,
  scalar/pointer/array/record type metadata, `dbg.declare`, and source locations.
- Out: DWARF v5 extensions, split DWARF, and optimized debug info quality.

## Deliverables

- Debug info wrapper in `rcc_codegen_llvm`.
- `-g` driver flag wiring if not already present.
- `llvm-dwarfdump` smoke tests gated on tool availability.

## Acceptance

- `rcc -g hello.c -o hello` emits an object with `.debug_info`.
- Without `-g`, debug sections are absent.
- Function names, parameter names, local names, and line records are visible.

## References

- LLVM Source Level Debugging
- DWARF v4 specification
