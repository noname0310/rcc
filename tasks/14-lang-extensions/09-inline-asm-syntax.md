# 14-09: Inline assembly syntax parsing

**Phase:** 14-lang-extensions    **Depends on:** 05-39    **Milestone:** M7 (stretch goal)

## Goal
Validate and refine the GCC-style inline assembly parser surface
introduced by task 05-39 before LLVM inline-asm lowering is added.

## Scope
- In: extension-mode diagnostics and operand-shape validation for the
  `InlineAsm` AST produced by task 05-39.
- Out: initial parser AST node and syntax recognition (task 05-39);
  codegen to LLVM inline assembly (task 14-10).
  Microsoft-style `__asm { ... }` syntax (out of scope).

## Deliverables
- Validation helpers for the `InlineAsm` AST produced by task 05-39.
- Tests connecting syntax to extension diagnostics.
- Tests: parse typical x86-64 inline asm statements.

## Acceptance
- `__asm__ volatile ("nop")` parses to an `InlineAsm` node.
- `asm("mov %1, %0" : "=r"(out) : "r"(in))` parses with correct
  output/input operand lists.
- Missing colons / malformed constraints produce diagnostics.

## References
- GCC inline assembly HOWTO.
- GCC extended asm syntax documentation.
