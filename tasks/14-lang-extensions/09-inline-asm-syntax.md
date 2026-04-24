# 14-09: Inline assembly syntax parsing

**Phase:** 14-lang-extensions    **Depends on:** —    **Milestone:** M7 (stretch goal)

## Goal
Parse GCC-style inline assembly statements:
`__asm__("template" : outputs : inputs : clobbers)` and the
`asm(...)` shorthand. Build a `StmtKind::InlineAsm` AST node
that captures the template string, output/input operand
constraints, and clobber list.

## Scope
- In: parser support for `asm` / `__asm__` / `__asm` keywords,
  colon-separated operand lists, string literal templates,
  `volatile` qualifier. Build AST node only — no codegen.
- Out: codegen to LLVM inline assembly (task 14-10).
  Microsoft-style `__asm { ... }` syntax (out of scope).

## Deliverables
- `StmtKind::InlineAsm` AST variant.
- Parser rules for the GCC extended asm syntax.
- Tests: parse typical x86-64 inline asm statements.

## Acceptance
- `__asm__ volatile ("nop")` parses to an `InlineAsm` node.
- `asm("mov %1, %0" : "=r"(out) : "r"(in))` parses with correct
  output/input operand lists.
- Missing colons / malformed constraints produce diagnostics.

## References
- GCC inline assembly HOWTO.
- GCC extended asm syntax documentation.
