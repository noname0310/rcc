> ✓ done — 2026-05-05

# 14-07: `__attribute__` semantic validation

**Phase:** 14-lang-extensions    **Depends on:** 05-38    **Milestone:** M5

## Goal
Validate and wire the GCC-style `__attribute__((...))` AST surface
introduced by task 05-38 into the extension-policy and semantic
pipeline.

## Scope
- In: extension-mode diagnostics, attachment-site validation,
  normalisation of attribute names/arguments, and semantic handoff for
  attributes already represented in the AST by task 05-38.
- Out: initial parser surface for attachment sites (task 05-38);
  semantic handling of any specific attribute (task 14-08).

## Deliverables
- Attribute validation scaffolding attached to the AST nodes from
  task 05-38.
- Tests that the parser surface feeds the phase-14 semantic layer.
- Tests that `packed`, `aligned(16)`, `section("text")`, and `unused`
  are preserved as normalised attribute records before specific
  semantics run.

## Acceptance
- The semantic layer receives `Attribute` records from declaration
  specifiers, declarators, tags, enumerators, and statements.
- Unknown attributes have a documented policy (ignore with warning,
  preserve for codegen, or reject).
- Site validation diagnostics do not require parser changes.

## References
- GCC attribute syntax documentation.
- C23 `[[...]]` syntax (future — out of scope here).
