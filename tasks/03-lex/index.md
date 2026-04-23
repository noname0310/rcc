# 03-lex: index

Build the full C99 preprocessing-token lexer with fuzz + corpus tests.

## Upstream deps

- 02-diagnostics

## Tasks (pick in order)

- [x] [01-cursor-iteration](01-cursor-iteration.md)
- [x] [02-line-splicing](02-line-splicing.md)
- [x] [03-whitespace-comments](03-whitespace-comments.md)
- [x] [04-identifier-universal-char](04-identifier-universal-char.md)
- [x] [05-pp-number](05-pp-number.md)
- [ ] [06-char-literal](06-char-literal.md)
- [ ] [07-string-literal](07-string-literal.md)
- [ ] [08-punctuator-table](08-punctuator-table.md)
- [ ] [09-header-name-context](09-header-name-context.md)
- [ ] [10-unit-tests-tables](10-unit-tests-tables.md)
- [ ] [11-unit-tests-ctestsuite-corpus](11-unit-tests-ctestsuite-corpus.md)
- [ ] [12-fuzz-target](12-fuzz-target.md)
- [ ] [13-snapshot-tokens-emit](13-snapshot-tokens-emit.md)

## Downstream

- 04-preprocess, 05-parse, 12-fuzz-differential
