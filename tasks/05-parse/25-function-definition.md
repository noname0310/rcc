# 05-25: Function definition

**Phase:** 05-parse    **Depends on:** 05-19, 05-13    **Milestone:** M2

## Goal
Recognise a top-level function definition: `decl-specs declarator
decl-list? compound-statement`. Distinguish from a declaration by
the presence of `{` after the declarator (and optional K&R decls —
task 26).

## Scope
- In: top-level parser peeks after the declarator; `{` → function
  definition; `;` or `,` → declaration.
- Out: K&R-style param declarations (task 26).

## Deliverables
- `parse_external_decl()` returning `ExternalDecl::Function` or
  `ExternalDecl::Decl`.
- Tests: prototype decl, full definition, parameter-less `void f(void)`.

## Acceptance
- `int main(void) { return 0; }` parses into a `FunctionDef` with
  the expected body.
- A stray `int x = 0;` next to a function parses cleanly.

## References
- C99 §6.9.1.
