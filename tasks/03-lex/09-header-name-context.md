# 03-09: Header-name context

**Phase:** 03-lex    **Depends on:** 03-07    **Milestone:** M5 prep

## Goal
Emit `PpTokenKind::HeaderName` **only when the lexer knows it is
inside an `#include` directive** (see C99 §6.4p4). Elsewhere `<`
starts a `Lt` punctuator. The simplest realisation is a one-shot
`expect_header_name` API the preprocessor calls after seeing
`# include`.

## Scope
- In: `Tokenizer::lex_header_name()` that reads either `"..."` or
  `<...>` producing one `HeaderName` token; normal loop **never**
  produces `HeaderName` spontaneously.
- Out: `#include` directive handling (that's 04-03).

## Deliverables
- Public method `lex_header_name(&mut self) -> Option<PpToken>`.
- Tests covering both `"stdio.h"` and `<stdio.h>` forms, plus
  malformed `<foo` with EOF → E0010.

## Acceptance
- Default tokenisation of `a < b` still yields `Ident`/`Lt`/`Ident`.
- Explicitly-invoked `lex_header_name` on `<stdio.h>` returns one
  `HeaderName` token.

## References
- C99 §6.4p4 / §6.4.7.
