# 03-lex

**Goal of the phase.** Turn the skeleton `rcc_lexer::Tokenizer` into a
full C99 preprocessing-token lexer (§6.4). Nothing in the skeleton
actually tokenises yet — it emits `PpTokenKind::Unknown` per byte.

## Tasks

| # | File | Summary |
|---|------|---------|
| 01 | [`01-cursor-iteration.md`](01-cursor-iteration.md) | Solid cursor primitives + offset math. |
| 02 | [`02-line-splicing.md`](02-line-splicing.md) | Handle `\\\n` continuation (phase 2). |
| 03 | [`03-whitespace-comments.md`](03-whitespace-comments.md) | Spaces, tabs, `//`, `/* ... */`. |
| 04 | [`04-identifier-universal-char.md`](04-identifier-universal-char.md) | Identifiers + `\\uXXXX` escapes. |
| 05 | [`05-pp-number.md`](05-pp-number.md) | Raw numeric preprocessing tokens. |
| 06 | [`06-char-literal.md`](06-char-literal.md) | `'c'`, `L'c'`, escapes. |
| 07 | [`07-string-literal.md`](07-string-literal.md) | `"..."`, `L"..."`, escapes. |
| 08 | [`08-punctuator-table.md`](08-punctuator-table.md) | Max-munch over C99 punctuators. |
| 09 | [`09-header-name-context.md`](09-header-name-context.md) | `<...>` header names (directive-only). |
| 10 | [`10-unit-tests-tables.md`](10-unit-tests-tables.md) | Table-driven tests per kind. |
| 11 | [`11-unit-tests-ctestsuite-corpus.md`](11-unit-tests-ctestsuite-corpus.md) | Lex every c-testsuite source; no panics. |
| 12 | [`12-fuzz-target.md`](12-fuzz-target.md) | `cargo fuzz run lex` 24 h no-panic. |
| 13 | [`13-snapshot-tokens-emit.md`](13-snapshot-tokens-emit.md) | `--emit=tokens` golden snapshots. |

## Exit criteria

- `rcc_lexer::tokenize` never panics on any byte sequence.
- Lexing a representative `c-testsuite` sample produces the exact
  token sequence in `tests/snapshots/...`.
- Fuzz target has a corpus under `fuzz/corpus/lex/` seeded from
  `third_party/testsuites/c-testsuite`.
