# 05-34: Decoded literal AST payloads

> ✓ done — 2026-05-01

**Phase:** 05-parse    **Depends on:** 05-33    **Milestone:** M2.1

## Goal
Stop downstream phases from re-decoding literal source text by carrying
the parser-level decoded literal payloads in the AST.

## Scope
- In:
  - Change literal expression AST variants to carry decoded
    `IntLiteral`, `FloatLiteral`, `CharLiteral`, and `StringLiteral`
    payloads or equivalent AST-owned payload types.
  - Update parser construction of literal expressions.
  - Update HIR lowering, typeck, CFG, codegen tests, and display
    helpers that currently read `ExprKind::*Lit { text }`.
  - Preserve source spelling where diagnostics still need it.
- Out:
  - New literal semantics beyond what the parser decoder already
    computes.

## Deliverables
- AST payload migration.
- Downstream match updates.
- Regression tests for integer suffixes, char encodings, adjacent
  strings, and float suffixes passing through AST/HIR.

## Acceptance
- Typeck/codegen no longer need to re-decode source slices for ordinary
  literal values.
- Existing literal diagnostics still point at the original source span.
- `cargo test --workspace` is green.
- Any intentional API break is documented in the task report.

## References
- `crates/rcc_parse/src/expr.rs` TODO about decoded payloads.
- `crates/rcc_parse/src/token.rs` decoded literal structs.
- `crates/rcc_ast/src/lib.rs::ExprKind`.
