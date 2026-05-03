> ✓ done — 2026-05-04

# 04-09: `#x` stringize operator

**Phase:** 04-preprocess    **Depends on:** 04-08    **Milestone:** M5

## Goal
Support `#parameter` inside a function-like macro body: the actual
argument's token sequence is converted into a string literal per C99
§6.10.3.2. Whitespace between tokens collapses to one space; string
literal `\\` and `"` inside argument tokens get escaped.

## Scope
- In: stringize before hide-set expansion (per C99 §6.10.3.2p2);
  E0024 if `#` is not followed by a parameter name.
- Out: token pasting (task 10).

## Deliverables
- `stringize(arg: &[PpToken]) -> PpToken` returning a `StringLit`.
- Tests: `#define S(x) #x / S(hello)` → `"hello"`; `S("a")` → `"\"a\""`.

## Acceptance
- Round-trip matches `cc -E` byte-for-byte on a small corpus.
- E0024 fires with a helpful note.

## References
- C99 §6.10.3.2.
