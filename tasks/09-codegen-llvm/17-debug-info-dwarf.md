# 09-17: DWARF debug info

**Phase:** 09-codegen-llvm    **Depends on:** 09-06    **Milestone:** M6    **Size:** Large

## Goal
Emit DWARF debug metadata via LLVM's DIBuilder API so that compiled
programs can be debugged with GDB/LLDB. Wire generation to the `-g`
CLI flag.

## Scope
- In: `DICompileUnit` per translation unit, `DIFile` per source
  file, `DISubprogram` per function definition, `DILocalVariable`
  per local variable with `dbg.declare` intrinsic, `DIBasicType`
  for scalar types, `DICompositeType` for structs/unions/arrays,
  `DIDerivedType` for pointers/typedefs/const/volatile.
  Attach `!dbg` location metadata to every LLVM instruction from
  source spans. `-g` flag enables generation, absence disables.
- Out: DWARF v5 extensions, split DWARF (`-gsplit-dwarf`),
  debug info for optimised code.

## Deliverables
- DIBuilder wrapper in `rcc_codegen_llvm`.
- Debug metadata for functions, locals, types.
- `!dbg` metadata on instructions.
- `-g` CLI flag wiring.
- Test: compile with `-g`, run `llvm-dwarfdump` to verify.

## Acceptance
- `rcc -g hello.c -o hello` produces an object with `.debug_info`.
- `llvm-dwarfdump` shows correct function names, parameter names,
  and local variable names.
- Line information allows `gdb`/`lldb` to set breakpoints by line.
- Without `-g`, no debug sections are emitted.

## References
- LLVM Source Level Debugging documentation.
- DWARF v4 specification.
- Clang's `CGDebugInfo.cpp` for reference.
