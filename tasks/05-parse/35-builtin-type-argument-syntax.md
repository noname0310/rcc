# 05-35: Builtin type-argument syntax

> ✓ done — 2026-05-01

**Phase:** 05-parse    **Depends on:** 05-34    **Milestone:** M5 blocker

## Goal
Parse GCC/Clang builtin forms whose argument grammar is not a normal
C function-call argument list, before phase 15 tries to lower them.

## Scope
- In:
  - Add parser support and AST representation for:
    - `__builtin_offsetof(type-name, member-designator)`
    - `__builtin_types_compatible_p(type-name, type-name)`
  - Keep ordinary expression-like builtins such as
    `__builtin_expect(expr, value)` as regular calls unless a stronger
    AST node is useful.
  - Parse member designators for offsetof: `.field`, `field`,
    nested fields, and array subscripts if chosen for compatibility.
  - Feature-gate GNU builtin syntax according to session options once
    extension options exist.
- Out:
  - Constant folding, layout lookup, type compatibility, or LLVM
    lowering.

## Deliverables
- AST node(s) for builtin type-argument syntax.
- Parser tests for `__builtin_offsetof(struct S, x)` and
  `__builtin_types_compatible_p(int, long)`.
- Negative tests for malformed type-name arguments.
- A scope note in `tasks/15-builtin-rt/06-builtin-common.md` that
  phase 15 depends on this parser surface.

## Acceptance
- `__builtin_offsetof(struct S, x)` parses without treating `struct S`
  as an expression argument.
- `__builtin_types_compatible_p(T, int *)` respects typedef-name
  lookup in both type-name slots.
- Malformed builtin type syntax produces parser diagnostics instead of
  silently becoming a broken call expression.

## References
- `tasks/15-builtin-rt/06-builtin-common.md`.
- GCC other builtins documentation.
- C99 type-name grammar.
