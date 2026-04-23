> ✓ done — 2026-04-23

# 05-13: Expression + block statements

**Phase:** 05-parse    **Depends on:** 05-12    **Milestone:** M1+

## Goal
Parse expression statements (`expr;`), null statements (`;`), and
compound statements (`{ BlockItem* }`).

## Scope
- In: `parse_stmt()` dispatcher; block-item list mixes declarations
  (task 18) and statements (this + 14/15/16/17); scope push/pop
  around each block.
- Out: control-flow statements (separate files).

## Deliverables
- `parse_stmt`, `parse_block`, `parse_block_item`.
- Tests: empty `{}`, `{ a; }`, `{ int x; x = 1; }`.

## Acceptance
- Nested blocks preserve scope stack correctness: the parser's
  ScopeStack depth after a top-level block is identical to before.

## References
- C99 §6.8.2, §6.8.3.
