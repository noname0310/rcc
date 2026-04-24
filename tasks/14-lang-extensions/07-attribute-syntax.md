# 14-07: `__attribute__` syntax parsing

**Phase:** 14-lang-extensions    **Depends on:** —    **Milestone:** M5

## Goal
Parse GCC-style `__attribute__((name))` and
`__attribute__((name(args)))` syntax. Attributes can appear in
declaration-specifier position and after declarators. Build an
`Attribute { name: Symbol, args: Vec<AttrArg> }` AST node attached
to the relevant declaration.

## Scope
- In: grammar extensions in `rcc_parse` for `__attribute__((...))`
  with arbitrarily nested parenthesised arguments. Attach parsed
  attributes to `DeclSpec` and `Declarator` AST nodes. Multiple
  comma-separated attributes inside one `__attribute__((...))`.
- Out: semantic handling of any specific attribute (task 14-08).

## Deliverables
- `Attribute` and `AttrArg` AST types.
- Parser rules for `__attribute__` in declaration specifiers and
  after declarators.
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
