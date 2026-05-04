# 11-06: GNU binary integer literals

**Phase:** 11-conformance    **Depends on:** 11-05    **Milestone:** M2+

## Goal
Accept GNU `0b...` / `0B...` integer literals behind an explicit GNU
compatibility option so chibicc `arith.c` can be triaged past its first
literal blocker.

## Scope
- In:
  - Add an `Options` flag for binary integer literals, default `false`.
  - Wire a driver flag or chibicc conformance-mode option that enables it.
  - Decode `0b1010`, `0B1010u`, and suffix combinations consistently with
    existing integer literal handling.
  - Preserve strict C99 behavior by rejecting binary literals unless the GNU
    option is enabled.
- Out:
  - Statement-expression semantics.
  - Any other GNU integer extensions.

## Deliverables
- Literal tests for strict rejection and GNU acceptance.
- Driver or conformance adapter wiring.
- Error-code documentation if a new diagnostic code is needed.

## Acceptance
- Strict `rcc --emit=ast` rejects `int x = 0b10;` with a clear diagnostic.
- GNU-enabled mode accepts `0b10011` and gives value 19.
- chibicc `arith.c` no longer fails first on `E0011 invalid digit 'b' in
  octal literal`.

## References
- chibicc `test/arith.c`.
- GCC binary constants extension.
