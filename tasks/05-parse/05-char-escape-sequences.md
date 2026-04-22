# 05-05: Char literal escape decoder

**Phase:** 05-parse    **Depends on:** 05-01    **Milestone:** M2

## Goal
Turn a `CharConst { enc }` pp-token into a `CharLiteral { value, encoding }`
by interpreting escape sequences per C99 §6.4.4.4.

## Scope
- In: simple escapes (`\n=10`, `\t=9`, ...); octal `\NNN` up to 3
  digits; hex `\xHH...` arbitrary length; UCN `\uXXXX` and
  `\UXXXXXXXX`; multi-char constants → implementation-defined (we
  pack bytes big-endian and emit warning W0003).
- Out: conversion to execution character set (we stay in Unicode
  code points).

## Deliverables
- `decode_char(raw: &str, enc: StringEncoding) -> Result<CharLiteral, Diagnostic>`.
- Tests: `'a'`, `'\n'`, `'\xff'`, `'\123'`, `'\u00e9'`, `'\Uxxxx'` (err).

## Acceptance
- `'\\0'` decodes to `0u32`.
- W0003 fires on `'ab'`.

## References
- C99 §6.4.4.4.
