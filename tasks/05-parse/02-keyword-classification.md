# 05-02: Keyword classification

**Phase:** 05-parse    **Depends on:** 05-01    **Milestone:** M1+

## Goal
Classify each identifier pp-token as either a reserved `Keyword` or a
plain `Ident(Symbol)`. Uses the existing `KEYWORDS` static table.

## Scope
- In: O(1) hash lookup; build a `OnceLock<FxHashMap<&str, Keyword>>`
  at first call.
- Out: context-sensitive keywords (none in C99 — `inline` is a real
  keyword at all positions).

## Deliverables
- `classify_ident(s: &str) -> Option<Keyword>`.
- Unit test iterating every entry in `KEYWORDS`.

## Acceptance
- All 37 C99 keywords round-trip.
- Non-keyword identifier (`printf`) returns `None`.

## References
- C99 §6.4.1.
