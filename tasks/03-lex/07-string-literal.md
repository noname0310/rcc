# 03-07: String literals

**Phase:** 03-lex    **Depends on:** 03-06    **Milestone:** M1

## Goal
Recognise `"..."`, `L"..."`, `u"..."`, `U"..."`, `u8"..."` string
literals with the same escape alphabet as character constants.
Adjacent-literal concatenation is a phase-6 (parser) job, not here.

## Scope
- In: encoding prefix; multi-line body only via line splicing (already
  handled in task 02); unterminated literal E0008.
- Out: concatenation, decoding to `Vec<u8>` (phase 05).

## Deliverables
- Lexer branch for string literals.
- Fixture tests for each prefix + edge cases (embedded `\0`, UTF-8
  continuation bytes in a narrow string — preserved verbatim).

## Acceptance
- Lexing `L"hi\\n" "bye"` yields two `StringLit` tokens (concat not
  performed at this layer).
- Unterminated `"hi` at EOF produces E0008.

## References
- C99 §6.4.5.
