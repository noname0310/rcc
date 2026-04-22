# 03-01: Cursor primitives

**Phase:** 03-lex    **Depends on:** —    **Milestone:** M1

## Goal
Harden `rcc_lexer::Cursor` to be the single source of truth for
byte-offset math during lexing. Every other task in phase 03 depends
on it, so it gets its own micro-task.

## Scope
- In: `bump_if`, `bump_while`, `peek_at(n)`; guarantee the cursor
  increments by valid UTF-8 character widths; unit tests for EOF
  behaviour and multi-byte characters.
- Out: line splicing (task 02).

## Deliverables
- `crates/rcc_lexer/src/cursor.rs` additions.
- `crates/rcc_lexer/tests/cursor.rs` covering empty input, 1-byte
  input, mixed ASCII + multibyte.

## Acceptance
- 100 % branch coverage on `cursor.rs` via `cargo llvm-cov`.
- Proptest-based roundtrip: "offset after bumping N chars equals the
  sum of their UTF-8 widths" (≥ 1000 random UTF-8 strings).

## References
- rustc's `rustc_lexer::Cursor`.
