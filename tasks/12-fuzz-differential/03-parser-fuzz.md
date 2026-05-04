# 12-03: Parser fuzz

**Phase:** 12-fuzz-differential    **Depends on:** 05-30    **Milestone:** M2+

## Goal
Add a parse-only fuzz target: bytes → lex → preprocess → parse.
Useful once the parser is feature-complete.

## Scope
- In: `fuzz/fuzz_targets/parse.rs`; seed from c-testsuite + chibicc.
- Out: differential comparisons (task 04).

## Deliverables
- Target + seed script.

## Acceptance
- No panics in a 30 minute path-filtered or manually dispatched run.
- Parsed token count per byte within a sane range (diagnostics
  guard against pathological blow-ups).

## References
- Plan §8.5.
