> ✓ done — 2026-04-23

# 03-10: Table-driven unit tests per token kind

**Phase:** 03-lex    **Depends on:** 03-03 .. 03-08    **Milestone:** M1

## Goal
One test file per `PpTokenKind` category so failures localise to the
exact lexer branch.

## Scope
- In: `crates/rcc_lexer/tests/idents.rs`, `numbers.rs`, `strings.rs`,
  `chars.rs`, `puncts.rs`, `comments.rs` — each a
  `#[test] fn table() { ... }` that iterates a `&[(&str, &[ExpectedTok])]`.
- Out: corpus-wide sanity (task 11).

## Deliverables
- 6 test files.
- Helper `lex_all(src: &str) -> Vec<(PpTokenKind, &str)>` in
  `crates/rcc_lexer/src/test_util.rs` (behind `#[cfg(test)]`).

## Acceptance
- Each test file contains ≥ 10 positive + ≥ 3 negative cases.
- `cargo test -p rcc_lexer` runtime < 200 ms.

## References
- Plan §8.2 "rcc_lexer: 토큰 종류별 테이블 주도 테스트".
