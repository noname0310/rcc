# 05-38: Attribute syntax surface

> ✓ done — 2026-05-01

**Phase:** 05-parse    **Depends on:** 05-37    **Milestone:** M5 blocker

## Goal
Move GCC-style `__attribute__((...))` parsing into the parser phase so
later extension tasks can focus on attribute semantics instead of
grammar surgery.

## Scope
- In:
  - Parse attributes in declaration specifier, declarator, type, field,
    enum, function, and statement positions that the project chooses
    to support.
  - Add an AST representation that preserves attribute name, argument
    token/expression payload, span, and attachment site.
  - Decide how unknown attributes are represented and whether strict
    C99 mode rejects or warns.
  - Update phase-14 attribute tasks to depend on this parser surface.
- Out:
  - Semantic handling of `packed`, `aligned`, `noreturn`, `section`,
    or target-specific attributes.

## Deliverables
- Attribute AST nodes and parser helpers.
- Tests for `__attribute__((packed))`,
  `__attribute__((aligned(16)))`, and
  `__attribute__((section("text"), unused))`.
- UI tests for malformed attribute parentheses.

## Acceptance
- Attribute syntax parses in every documented attachment site.
- Attribute payload is preserved enough for phase 14 semantics to
  validate argument count and type.
- Existing strict C99 fixtures remain green.

## Notes
- The AST now preserves GNU attributes on `DeclSpecs`, `Declarator`,
  `RecordSpec`, `EnumSpec`, `Enumerator`, and `StmtKind::Attributed`.
- Strict C99 mode parses attributes but emits W0015 unless
  `Options::gnu_attributes` is enabled.
- Attribute semantics remain deferred to
  `tasks/14-lang-extensions/07-attribute-syntax.md` and the
  attribute-specific follow-up tasks.

## References
- `tasks/14-lang-extensions/07-attribute-syntax.md`.
- GCC attribute syntax documentation.
