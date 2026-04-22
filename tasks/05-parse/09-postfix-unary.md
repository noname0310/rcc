# 05-09: Postfix / unary operators

**Phase:** 05-parse    **Depends on:** 05-08    **Milestone:** M1+

## Goal
Handle the non-binary parts of §6.5.2 / §6.5.3: `a[b]`, `a.b`, `a->b`,
`a++`, `a--`, `f(args)`, prefix `++a`, `--a`, `+a`, `-a`, `~a`, `!a`,
`*a`, `&a`.

## Scope
- In: postfix loop after a primary; prefix chain inside
  `parse_expr_bp` branch.
- Out: `sizeof` (task 10), cast (task 10), compound literal (task 22
  in phase 05, actually task 10 here).

## Deliverables
- `parse_postfix(base: Expr) -> Expr`.
- `parse_prefix_unary() -> Expr`.
- Fixture tests: chain `f(a)[b].c->d++`.

## Acceptance
- `a.b->c[0]++` parses in expected left-to-right associativity.
- `&*&x` parses as three nested `AddressOf` / `Deref`.

## References
- C99 §6.5.2, §6.5.3.
