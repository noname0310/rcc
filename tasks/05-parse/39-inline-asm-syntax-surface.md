# 05-39: Inline asm syntax surface

> ✓ done — 2026-05-01

**Phase:** 05-parse    **Depends on:** 05-38    **Milestone:** M5 blocker

## Goal
Parse GCC-style inline assembly statements before codegen extension
work needs to lower them.

## Scope
- In:
  - Parse `asm`, `__asm`, and `__asm__` spellings as extension syntax.
  - Support basic and extended asm:
    `asm("template")` and
    `asm volatile ("template" : outputs : inputs : clobbers)`.
  - Preserve string templates, operand constraints, operand
    expressions, clobber strings, qualifiers, and source spans.
  - Gate under GNU extension mode.
- Out:
  - Constraint validation.
  - Register allocation / LLVM `call asm` lowering.
  - Microsoft `__asm { ... }` block syntax.

## Deliverables
- AST statement node for inline asm.
- Parser tests for basic asm, volatile asm, outputs, inputs, and
  clobber lists.
- Malformed asm UI tests.
- Scope note in phase-14 codegen task that parser syntax is complete.

## Acceptance
- `__asm__ volatile ("nop")` parses to a stable AST node.
- `asm("mov %1, %0" : "=r"(out) : "r"(in) : "cc")` preserves every
  operand and clobber.
- Strict C99 mode rejects or warns according to extension policy.

## Notes
- The AST now preserves GNU inline asm as `StmtKind::InlineAsm` with
  qualifiers, template, output/input operands, constraints, clobbers,
  and source spans.
- Strict C99 mode parses inline asm but emits W0016 unless
  `Options::gnu_inline_asm` is enabled.
- Constraint validation and LLVM inline-asm lowering remain deferred to
  `tasks/14-lang-extensions/09-inline-asm-syntax.md` and follow-up
  codegen tasks.

## References
- `tasks/14-lang-extensions/09-inline-asm-syntax.md`.
- LLVM inline asm lowering requirements.
