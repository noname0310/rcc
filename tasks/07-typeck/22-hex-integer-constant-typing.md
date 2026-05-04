# 07-22: hex integer constant typing

> ✓ done — 2026-05-04

**Phase:** 07-typeck    **Depends on:** 07-08, 07-14, 09-29    **Milestone:** M6+

## Goal
Implement C99 integer constant type selection for hexadecimal and octal
constants so unsigned-valued literals do not get sign-extended through calls.

## Trigger
- After aggregate `va_arg` is fixed, `c-testsuite::00204` runs but prints
  `ffffffffabcd0000` for `pll(0xabcd0000)` instead of `abcd0000`.

## Scope
- In:
  - Review lexer/parser literal metadata for integer base and suffix.
  - Apply C99 §6.4.4.1 candidate type lists for decimal vs octal/hex
    constants.
  - Ensure prototype conversions to `unsigned long long` zero-extend values
    whose source type is `unsigned int`.
  - Add regression coverage for `0xabcd0000`, `0xffffabcd`, and suffixed
    variants.
- Out:
  - Non-C99 binary integer literals (`0b...`).
  - Preprocessor `#if` integer model changes unless the same bug is proven
    there.

## Deliverables
- Typeck/const-eval tests for unsuffixed hexadecimal constants crossing
  `INT_MAX`.
- Focused executable regression for the `pll(0xabcd0000)` shape.
- Updated conformance dashboard if `00204` becomes fully green.

## Acceptance
- [x] `0xabcd0000` is typed as `unsigned int` on LP64.
- [x] Passing `0xabcd0000` to `unsigned long long` zero-extends to
  `0x00000000abcd0000`.
- [x] Existing decimal integer literal behavior remains unchanged.
- [x] `c-testsuite::00204` passes fully.

## Result
- Parser phase 7 now preserves decimal/octal/hex base metadata.
- HIR keeps integer literals as pre-typeck `IntLiteral` nodes until typeck
  applies the C99 §6.4.4.1 candidate lists.
- Latest c-testsuite report: 215 pass, 1 fail, 4 xfail, 0 skip.

## References
- C99 §6.4.4.1 integer constants
- `third_party/testsuites/c-testsuite/tests/single-exec/00204.c`
