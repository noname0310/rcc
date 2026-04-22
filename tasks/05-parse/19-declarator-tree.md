# 05-19: Declarator tree

**Phase:** 05-parse    **Depends on:** 05-18    **Milestone:** M1+

## Goal
Parse a declarator: name + nested pointer / array / function chain
captured as a `Declarator` with a `Vec<DerivedDeclarator>`. The
ordering matters (inside-out); HIR lowering will fold these into a
`Ty`.

## Scope
- In: handle `*T`, `T[N]`, `T(args)`, nested `(*fp)(int)`; preserve
  order as encountered in source.
- Out: abstract declarator (task 20).

## Deliverables
- `parse_declarator() -> Declarator`.
- Fixtures covering each C99 declarator form from §6.7.5.

## Acceptance
- `int (*fp[3])(int, int)` parses with the chain `Array(3) →
  Pointer → Function((int,int))` in reverse-syntactic order.
- `int a[10][20]` parses with two `Array` derivations.

## References
- C99 §6.7.5.
