# 15a-10: Unicode Character and String Literals

**Phase:** 15a-c11-transition  
**Depends on:** 15a-02-c11-keyword-tokenization  
**Milestone:** c11-transition

## Goal

Implement the C11 Unicode literal surface and `<uchar.h>` typedefs without
breaking existing narrow/wide string handling.

## Scope

- In: lexer/parser support for `u'c'`, `U'c'`, `u"..."`, `U"..."`, and
  `u8"..."` spellings.
- In: HIR types for `char16_t` and `char32_t` literals through resource
  typedefs or builtin scalar aliases.
- In: string literal concatenation rules across compatible encodings.
- In: `<uchar.h>` declarations for `char16_t`, `char32_t`, `mbrtoc16`,
  `c16rtomb`, `mbrtoc32`, and `c32rtomb`.
- Out: full Unicode normalization or locale conversion implementation.

## Acceptance

- [ ] Unicode-prefixed literals tokenize and parse in C11 mode.
- [ ] Invalid mixed string literal concatenations are diagnosed.
- [ ] `sizeof(u"x"[0])` and `sizeof(U"x"[0])` match the selected target types.
- [ ] `<uchar.h>` resource header parses and lowers.

## References

- N1570 6.4.4.4 character constants.
- N1570 6.4.5 string literals.
- N1570 7.28 `uchar.h`.
