# 05-31: Parser recovery contract

**Phase:** 05-parse    **Depends on:** 05-30    **Milestone:** M2.1

## Goal
Make parser error recovery predictable enough that HIR lowering, CFG,
and conformance smoke tests can rely on the parser to preserve every
valid declaration or statement after a malformed one.

## Scope
- In:
  - Define a cursor-progress contract for `parse_declaration`,
    `parse_external_decl`, `parse_block_item`, and expression recovery.
  - Fix declaration paths where specifiers are consumed before a
    declarator failure and the caller cannot recover cleanly.
  - Add regression tests for malformed declarations at file scope,
    block scope, `for` init scope, parameter lists, and K&R declaration
    lists.
  - Ensure recovery never leaks parser scopes.
- Out:
  - Semantic diagnostics such as unknown identifiers, invalid break
    targets, duplicate labels, or invalid lvalues.

## Deliverables
- Recovery tests in `crates/rcc_parse/tests/grammar.rs` or a dedicated
  `recovery.rs`.
- Focused parser fixes in `crates/rcc_parse/src/{lib,decl,stmt,expr}.rs`.
- A short recovery invariant comment near `Parser::recover_to_sync`.

## Acceptance
- A bad declaration followed by a valid declaration still yields the
  valid declaration at file scope and inside blocks.
- A bad `for (int ...` init pops the `for` scope before returning.
- A bad K&R declaration list cannot spin and still attempts to parse
  the following function body.
- `cargo test -p rcc_parse recovery` is green.
- `cargo test -p rcc_parse ctestsuite_parse_smoke --test ctestsuite_smoke`
  remains green.

## References
- C99 §6.7, §6.8, §6.9.
- `crates/rcc_parse/src/lib.rs::Parser::recover_to_sync`.
- Review finding: declaration parser consumes specifiers before some
  hard failures, which can hide later valid syntax from callers.
