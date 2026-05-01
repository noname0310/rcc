# 05-36: GNU statement-expression syntax

> ✓ done — 2026-05-01

**Phase:** 05-parse    **Depends on:** 05-35    **Milestone:** M5 blocker

## Goal
Parse GNU statement expressions `({ block-item* expr? ; })` so
real-world GNU-flavoured tests no longer fail before HIR lowering.

## Scope
- In:
  - Add AST representation for a statement expression.
  - Extend expression parsing to recognise `({ ... })` before ordinary
    parenthesised expression / cast disambiguation consumes it.
  - Reuse block-item parsing so declarations, labels, loops, and
    nested blocks inside statement expressions share normal statement
    semantics.
  - Add session option / extension gate if not already available.
  - Add parse-only tests from c-testsuite `00213` and `00214` reduced
    fixtures.
- Out:
  - Value category, result type, lifetime, or codegen semantics.
  - Optimising code suppression around labels; that belongs to HIR/CFG.

## Deliverables
- AST node and parser branch.
- Grammar tests and UI diagnostics for malformed statement expressions.
- xfail reason updates for affected c-testsuite files.

## Acceptance
- `int x = ({ int y = 1; y; });` parses.
- Labels and gotos inside a statement expression are preserved in the
  AST for later CFG work.
- In strict C99 mode, the construct is rejected or warned according to
  the chosen extension policy.

## References
- GNU C statement expressions.
- `third_party/testsuites/c-testsuite/tests/single-exec/00213.c`.
- `third_party/testsuites/c-testsuite/tests/single-exec/00214.c`.
