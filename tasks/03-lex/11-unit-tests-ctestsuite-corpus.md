# 03-11: Corpus-wide sanity against c-testsuite

**Phase:** 03-lex    **Depends on:** 03-10, 01-01    **Milestone:** M1

## Goal
Run the lexer over **every** source file in
`third_party/testsuites/c-testsuite/tests/single-exec/` and assert:
1. No panics.
2. Token spans partition the input bytes exactly (with the
   line-splice exception explained in task 02).
3. No `PpTokenKind::Unknown` outside of sequences pre-declared
   in a small allow-list (`$`, `@`, `` ` ``).

## Scope
- In: `crates/rcc_lexer/tests/corpus.rs` reading the suite directory
  at test time (use an env var or a build-script discovered path);
  skip cleanly when the suite is not vendored.
- Out: preprocessing / parsing those files (later phases).

## Deliverables
- `corpus.rs` that iterates `*.c` and runs the three assertions.
- Helpful failure output: when an assertion fails, print the
  offending byte slice + `PpTokenKind`.

## Acceptance
- `cargo xtask fetch-testsuites --only c-testsuite` then
  `cargo test -p rcc_lexer --test corpus`: green on ≥ 200 files.
- Test is gated on existence of the suite (won't break local dev
  without internet).

## References
- Plan §9.4 "PR 단위: c-testsuite 전체".
