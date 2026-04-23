> ✓ done — 2026-04-23

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

## Notes (agent)
- Error code: the task scope names E0031, but the existing
  preprocessor registry already reserves `E0020` with the exact
  description "#error directive encountered" (see
  `crates/rcc_errors/src/codes.rs`). Using `E0020` keeps the
  reserved slot honoured; no new `E` code was needed.
- Warning codes: the registry previously held only `EXXXX` codes.
  Task 04-16 introduces a separate `WXXXX` namespace starting at
  `W0001` for unknown `#pragma`. `ALL_CODES` now spans both
  prefixes; the `codes_are_sorted` test was split per-namespace.
- Halt: a new `Preprocessor::halted` latch short-circuits the
  main `run()` loop (and suppresses the end-of-file E0018 sweep)
  on the first `#error`, matching GCC/clang's fatal semantics for
  §6.10.5.
