# 14-10: Inline assembly codegen

**Phase:** 14-lang-extensions    **Depends on:** 14-09    **Milestone:** M7 (stretch goal)

## Goal
Lower `StmtKind::InlineAsm` to LLVM inline assembly via
`InlineAsm::get()`. Map GCC-style operand constraints to LLVM
constraint strings and wire output/input operands.

## Scope
- In: translate the parsed inline asm AST to an LLVM
  `call asm` instruction. Map common GCC constraints (`"r"`,
  `"m"`, `"i"`, `"=r"`, `"+r"`) to their LLVM equivalents.
  Handle `volatile` asm. Clobber list → LLVM clobber constraints.
- Out: target-specific constraint validation (accept all and let
  LLVM reject invalid ones for now).

## Deliverables
- `InlineAsm` emission in `rcc_codegen_llvm`.
- Constraint string translation.
- Tests: simple `nop`, register move, memory operand.

## Acceptance
- `__asm__ volatile ("nop")` produces `call void asm sideeffect "nop", ""()`.
- An asm with output operand produces a valid LLVM `call asm`
  with the correct result type.
- LLVM module verification passes for emitted inline asm.

## References
- LLVM Language Reference: Inline Assembler Expressions.
- GCC to LLVM constraint mapping.
