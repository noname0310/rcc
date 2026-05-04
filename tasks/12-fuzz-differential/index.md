# 12-fuzz-differential: index

Long-running oracle verification: cargo-fuzz per front-end crate + csmith differential vs host cc.

## Upstream deps

- 03-lex (per-target), 04-preprocess, 05-parse, 09-codegen-llvm

## Tasks (pick in order)

- [x] [01-lexer-fuzz-30m](01-lexer-fuzz-30m.md)
- [x] [02-preprocess-fuzz](02-preprocess-fuzz.md)
- [x] [03-parser-fuzz](03-parser-fuzz.md)
- [x] [04-csmith-differential-harness](04-csmith-differential-harness.md)
- [ ] [05-csmith-bounded-differential](05-csmith-bounded-differential.md)

## Downstream

- 13-quality (release gate)
