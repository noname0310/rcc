# 04-16: `#error` and unknown `#pragma`

**Phase:** 04-preprocess    **Depends on:** 04-02    **Milestone:** M5

## Goal
Honour `#error` (diagnostic + abort the compilation unit) and
gracefully ignore every `#pragma` we don't recognise (already
handled: `once`, `STDC *`).

## Scope
- In: `#error <tokens...>` emits E0031 with the tokens stringised;
  `#pragma STDC FP_CONTRACT ON` etc. are accepted silently per
  §6.10.6; unknown `#pragma` emits a warning W0001 but proceeds.
- Out: implementation-defined pragmas — document how to add them
  later.

## Deliverables
- Handler in directive dispatcher.
- Tests: `#error foo bar` produces E0031 with message `foo bar`;
  `#pragma mystery` produces W0001.

## Acceptance
- Compilation halts at the first `#error`; subsequent directives
  are not processed.
- W0001 does **not** count as an error for `Handler::has_errors`.

## References
- C99 §6.10.5 (`#error`), §6.10.6 (`#pragma`).
