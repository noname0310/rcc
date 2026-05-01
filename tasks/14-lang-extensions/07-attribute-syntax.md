# 14-07: `__attribute__` syntax parsing

**Phase:** 14-lang-extensions    **Depends on:** 05-38    **Milestone:** M5

## Goal
Validate and wire the GCC-style `__attribute__((...))` parser surface
introduced by task 05-38 into the extension-policy and semantic
pipeline.

## Scope
- In: extension-mode diagnostics, attachment-site validation, and
  semantic handoff for attributes already represented in the AST by
  task 05-38. Multiple comma-separated attributes inside one
  `__attribute__((...))`.
- Out: initial parser surface for attachment sites (task 05-38);
  semantic handling of any specific attribute (task 14-08).

## Deliverables
- Attribute validation scaffolding attached to the AST nodes from
  task 05-38.
- Tests that the parser surface feeds the phase-14 semantic layer.
- Tests: parse `__attribute__((packed))`,
  `__attribute__((aligned(16)))`,
  `__attribute__((section("text"), unused))`.

## Acceptance
- `int x __attribute__((aligned(16)));` parses successfully and
  the AST carries the attribute.
- `__attribute__((a, b(1,2)))` produces two `Attribute` nodes.
- Malformed attributes produce a diagnostic.

## References
- GCC attribute syntax documentation.
- C23 `[[...]]` syntax (future — out of scope here).
