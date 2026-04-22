# 03-13: `--emit=tokens` golden snapshots

**Phase:** 03-lex    **Depends on:** 03-10    **Milestone:** M1

## Goal
Ship a driver stage that pretty-prints the pp-token stream and bolt
it to `insta` so every change in tokenisation is reviewed.

## Scope
- In: `rcc_driver::pipeline` renders `Tokens` when `opts.emit`
  contains `EmitKind::Tokens`; format: `LN:CO-LN:CO  <kind>  <text>`
  one per line; `crates/rcc_driver/tests/emit_tokens.rs` using
  `insta::assert_snapshot!`.
- Out: AST/HIR/MIR emit stages (later tasks).

## Deliverables
- Token pretty-printer in `rcc_lexer::pretty` module.
- ≥ 5 snapshot fixtures under
  `crates/rcc_driver/tests/snapshots/tokens/`.

## Acceptance
- `cargo test -p rcc_driver --test emit_tokens`: all snapshots
  reviewed + committed.
- Running `rcc --emit=tokens tests/fixtures/hello.c` prints a stable
  output byte-for-byte.

## References
- `insta` docs.
