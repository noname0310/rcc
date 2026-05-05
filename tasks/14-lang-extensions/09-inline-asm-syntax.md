> ✓ done — 2026-05-05

# 14-09: Inline assembly semantic validation

**Phase:** 14-lang-extensions    **Depends on:** 05-39    **Milestone:** M7 (stretch goal)

## Goal
Validate the GCC-style inline assembly AST surface introduced by task
05-39 before LLVM inline-asm lowering is added.

## Scope
- In: extension-mode diagnostics, operand-shape validation, constraint
  classification, and semantic handoff for the `InlineAsm` AST produced
  by task 05-39.
- Out: initial parser AST node and syntax recognition (task 05-39);
  codegen to LLVM inline assembly (task 14-10).
  Microsoft-style `__asm { ... }` syntax (out of scope).

## Deliverables
- Validation helpers for the `InlineAsm` AST produced by task 05-39.
- Tests connecting parsed templates, constraints, operands, clobbers,
  and qualifiers to extension diagnostics.
- Tests for typical x86-64 inline asm statements after parsing.

## Acceptance
- The semantic layer receives `InlineAsm` nodes from basic and extended
  asm statements.
- Constraint strings are classified or rejected without parser changes.
- Unsupported `asm goto` / target constraints have a documented policy.

## References
- GCC inline assembly HOWTO.
- GCC extended asm syntax documentation.
