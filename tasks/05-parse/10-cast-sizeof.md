> ✓ done — 2026-04-23

# 05-10: Cast and `sizeof`

**Phase:** 05-parse    **Depends on:** 05-20    **Milestone:** M1+

## Goal
Parse the two `(`-initial constructs that share a type-name lookahead
with compound literals: the cast expression `(T)e` (§6.5.4) and the
two `sizeof` shapes — `sizeof e` and `sizeof(T)` (§6.5.3.4). Compound
literal `(T){init}` has been split into the follow-up task
[`10b-compound-literal`](10b-compound-literal.md) so the `parse_prefix_
unary` layer can land without waiting on the initializer-list parser
(task 05-24).

## Scope
- In: extend `parse_prefix_unary` to recognise `sizeof` as a unary
  operator (both shapes) and to treat a `(` followed by a
  *type-name starter* (type-specifier keyword, type-qualifier, or a
  typedef-name recognised via `ScopeStack::is_typedef`) as the start
  of a cast `( type-name ) cast-expression`; otherwise fall through
  to the existing paren-expression path in `parse_primary`.
- In: produce `ExprKind::Cast { ty, expr }`,
  `ExprKind::SizeofExpr(e)`, and `ExprKind::SizeofType(T)` using the
  existing AST variants.
- Out: compound literal `( type-name ) { ... }` — see task 10b.
- Out: `_Alignof` (C11, not in C99).

## Key decision — `(` ambiguity

C99 makes `( something )` locally ambiguous between three shapes
that can appear at the start of a *unary-expression*:

```text
   ( ident )            →   paren if ident is NOT a typedef-name
                            cast   if ident IS  a typedef-name
   ( type-kw ... )      →   cast / sizeof type
   ( non-type-expr )    →   paren
```

The parser resolves this with one-token lookahead past the `(`:

- Type-specifier / type-qualifier keyword → definitely a type-name,
  so parse as cast (or, under `sizeof`, as `SizeofType`).
- Ordinary identifier + `ScopeStack::is_typedef(sym)` → typedef-name
  → type-name → cast.
- Anything else → drop through to `parse_primary`, which handles
  `( expression )`.

This is the same hook task 05-10b will extend for compound literals;
once `parse_type_name` has been called and the closing `)` consumed,
task 10b will peek at the next token and branch on `{` vs. anything
else.

## Deliverables
- `parse_prefix_unary` grows two new branches: one for
  `Keyword::Sizeof`, one for `( type-name )` — the latter via a
  helper that tests the lookahead and calls
  [`parse_type_name`](20-abstract-declarator.md).
- Tests (rcc_parse::expr::tests):
  - `(int)x` → `Cast { ty: int, expr: Ident(x) }`.
  - `sizeof x` → `SizeofExpr(Ident(x))`.
  - `sizeof(int)` → `SizeofType(int)`.
  - `sizeof(int*)` → `SizeofType(int *)`.
  - Typedef disambiguation: with `T` declared as `NameKind::Typedef`
    in the scope stack, `(T)x` parses as cast; with `T` declared
    `NameKind::Ordinary`, `(T)` parses as paren-group-of-ident.

## Acceptance
- Typedef ambiguity: `typedef int T; (T)x` parses as cast;
  `int T = 0; (T)` parses as paren.
- `sizeof(int)` returns `ExprKind::SizeofType` with `TypeName`
  specs `[Int]`; `sizeof x` returns `ExprKind::SizeofExpr`.
- Compound literal tests (`(int[3]){0}`) are **not** exercised here;
  they live in task 10b alongside the initializer-list dependency.

## References
- C99 §6.5.3.4 (`sizeof`), §6.5.4 (cast).
- Task [`20-abstract-declarator`](20-abstract-declarator.md) — the
  underlying `parse_type_name` call.
- Task [`10b-compound-literal`](10b-compound-literal.md) — split-out
  sibling that owns `( type-name ) { ... }`.

## Notes (agent)
- 2026-04-23: scope narrowed — compound literal `( T ){ init }` was
  split into the follow-up task `10b-compound-literal` because it
  depends on task 05-24 (init-list-designators), which is still
  `[ ]`. Cast + sizeof only need abstract declarators (task 05-20,
  now `[x]`), so this task runs on its own. Once task 05-24 lands,
  task 10b will finish the original bundle.
- Previous entry (2026-04-23, superseded by the above): skipped in
  favour of 05-11 — upstream deps 05-20 and 05-24 were still
  `[ ]`; 05-20 has since landed.
