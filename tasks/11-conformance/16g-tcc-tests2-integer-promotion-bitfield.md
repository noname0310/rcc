# 11-16g: tcc-tests2 integer promotion and bit-fields

> ✓ done — 2026-05-05 — `93_integer_promotion` now passes; WSL tcc-tests2 is 71 pass / 9 xfail / 4 fail / 4 skip.

**Phase:** 11-conformance    **Depends on:** 11-16    **Milestone:** M6

## Goal
Fix the integer-promotion bug exposed by bit-field and narrow-integer
expressions.

## Scope
- In: `tcc-tests2::93_integer_promotion`.
- Out: full bit-field layout policy; that is owned by 11-16h.

## Deliverables
- Typeck tests for integer promotions on bit-fields, `_Bool`, `char`, and
  small integer ranks.
- Codegen tests ensuring promoted values are sign/zero extended according to
  the promoted type.

## Acceptance
- `93_integer_promotion` passes through tcc-tests2.
- The fix does not regress gcc-torture bit-field precision cases.

## References
- `target/wsl/tcc-tests2-16-final.json`
- C99 §6.3.1.1.
