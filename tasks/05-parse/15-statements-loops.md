> ✓ done — 2026-04-23

# 05-15: `while`, `do-while`, `for`

**Phase:** 05-parse    **Depends on:** 05-13    **Milestone:** M1+

## Goal
Parse the three C99 iteration statements. `for` supports both
expression-init and declaration-init (C99 §6.8.5p3).

## Scope
- In: scope push for `for (decl; ...; ...)` init; parse init as
  `BlockItem` (`Decl` or `Stmt::Expr`).
- Out: `break`/`continue` target resolution (CFG phase).

## Deliverables
- Parsers for each form.
- Tests: `for(int i=0; i<10; i++)`, `do { ... } while(...);`.

## Acceptance
- `for (int i = 0; i < n; ++i)` parses with `i` visible *only* inside
  the loop (verified by follow-up `i;` outside raising an unknown-
  identifier diagnostic at the HIR stage — fixture deferred).

## References
- C99 §6.8.5.
