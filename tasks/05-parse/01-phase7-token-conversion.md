> ✓ done — 2026-04-23

# 05-01: Phase-7 token conversion

**Phase:** 05-parse    **Depends on:** 04-17    **Milestone:** M1+

## Goal
Implement C99 §5.1.1.2 phase 7: convert a preprocessed pp-token
stream into the parser's `Token` type. This fans out to the sub-tasks
for each literal flavour.

## Scope
- In: `pp_to_token(pp: PpToken) -> Token` switch; identifiers go to
  keyword classification (task 02), numbers to 03/04, char/string
  literals to 05/06; punctuators pass through.
- Out: the parser state machine (task 07+).

## Deliverables
- `crates/rcc_parse/src/phase7.rs` housing the driver.
- Unit test confirming every `PpTokenKind` has a matching branch
  (compile-fail test via `#[non_exhaustive]` is ok).

## Acceptance
- 1:1 pp-token → token conversion with span preservation.
- Adjacent strings are handled (task 06 swaps in a real impl).

## References
- C99 §5.1.1.2 phase 7.
