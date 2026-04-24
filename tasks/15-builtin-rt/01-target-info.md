# 15-01: `TargetInfo` struct

**Phase:** 15-builtin-rt    **Depends on:** —    **Milestone:** M5

## Goal
Create a `TargetInfo` struct that encapsulates all target-dependent
parameters: data model, pointer width, endianness, type sizes and
alignments, OS, and architecture. Provide a factory
`TargetInfo::from_triple(&TargetTriple)` that defaults to the host
triple. Replace hardcoded sizes in `rcc_codegen_llvm::LayoutCx`.

## Scope
- In: `TargetInfo` fields: `data_model` (LP64 / ILP32 / LLP64),
  `pointer_width` (32/64), `endianness` (little/big),
  `type_sizes` (size and alignment for `char`, `short`, `int`,
  `long`, `long long`, `float`, `double`, `long double`), `os`
  (Linux/macOS/Windows/None), `arch` (x86_64/aarch64/i386/...).
  `TargetTriple` parsing (arch-vendor-os-env).
  Default to host triple when none specified.
- Out: ABI calling-convention details (stays in codegen).

## Deliverables
- `rcc_target` crate (or module) with `TargetInfo`, `TargetTriple`,
  `DataModel`, `Endianness` types.
- `TargetInfo::from_triple()` supporting x86_64-linux-gnu,
  x86_64-apple-darwin, x86_64-pc-windows-msvc, aarch64-linux-gnu.
- Integration into `LayoutCx` replacing hardcoded constants.
- Unit tests verifying type sizes per data model.

## Acceptance
- `TargetInfo::from_triple("x86_64-unknown-linux-gnu")` reports
  `pointer_width == 64`, `long_size == 8` (LP64).
- `TargetInfo::from_triple("x86_64-pc-windows-msvc")` reports
  `long_size == 4` (LLP64).
- `LayoutCx` reads sizes from `TargetInfo`, not constants.

## References
- System V ABI, section 3.1.2 (type sizes).
- Microsoft x64 ABI documentation.
- LLVM `TargetInfo` / `DataLayout` design.
