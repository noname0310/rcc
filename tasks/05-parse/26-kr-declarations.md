# 05-26: K&R-style function definitions

**Phase:** 05-parse    **Depends on:** 05-25    **Milestone:** M6

## Goal
Accept old-style function definitions:

```c
int f(x, y) int x; double y; { ... }
```

C99 still allows these (§6.9.1p6) but deprecates them. Parse,
produce AST, emit W0005 "K&R function declaration is obsolete".

## Scope
- In: distinct branch in `parse_external_decl` when declarator's
  function derivation uses `kr_names` (identifier list) instead of
  `params`.
- Out: param-type inference (HIR lowering).

## Deliverables
- Parser branch + fixture based on real K&R code (e.g. pre-ANSI
  Bourne shell snippet).

## Acceptance
- Parses without error; emits W0005 with a `help: rewrite using
  prototype syntax`.
- A K&R decl list referencing a name that isn't in the identifier
  list is E0063.

## References
- C99 §6.9.1p6; K&R 1st edition syntax.
