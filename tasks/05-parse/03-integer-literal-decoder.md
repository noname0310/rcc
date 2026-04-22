# 05-03: Integer literal decoder

**Phase:** 05-parse    **Depends on:** 05-01    **Milestone:** M1+

## Goal
Decode a `PpNumberKind::Integer` pp-token into `IntLiteral { value, suffix }`.
Handle decimal, octal (`0NNN`), hexadecimal (`0xNNN`) bases and
suffixes `u`, `l`, `ll`, `ul`, `ull` (case-insensitive).

## Scope
- In: accept any casing; detect multiple same-case suffix letters;
  overflow on `u128` → E0040 "integer literal too large".
- Out: final type selection (C99 §6.4.4.1p5 ladder) — done in
  typeck task 07-01.

## Deliverables
- `decode_integer(text: &str) -> Result<IntLiteral, Diagnostic>`.
- Tests: `0`, `0xff`, `0777`, `1u`, `42ULL`, `0x1'000'000` (fails;
  digit separators are C++17, not C99).

## Acceptance
- All corner cases round-trip.
- E0040 is emitted with a span underlining the literal.

## References
- C99 §6.4.4.1.
