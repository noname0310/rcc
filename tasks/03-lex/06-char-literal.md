# 03-06: Character constants

**Phase:** 03-lex    **Depends on:** 03-04    **Milestone:** M1

## Goal
Recognise character constants `'c'`, `L'c'`, `u'c'`, `U'c'`, `u8'c'`
(C11 prefix supported for forward compatibility). Emit
`PpTokenKind::CharConst { enc }` spanning the complete literal.

## Scope
- In: encoding prefix detection; escape sequences (`\n`, `\t`, `\\`,
  `\'`, `\"`, `\?`, `\a`, `\b`, `\f`, `\r`, `\v`, octal `\NNN`,
  hex `\xHH+`, UCN `\uXXXX`/`\UXXXXXXXX`); multi-char constants
  (§6.4.4.4p10 — implementation-defined, keep bytes).
- Out: decoding the byte value (that's phase 05).

## Deliverables
- Lexer branch for character constants producing a single span.
- E0006 "unterminated character constant".
- E0007 "invalid escape sequence \\X".

## Acceptance
- `'a'`, `L'\xff'`, `'\\'`, `'\u0041'` each produce one
  `CharConst { enc }` token with the correct `enc` variant.
- Unterminated `'a` at EOF produces E0006.

## References
- C99 §6.4.4.4.
