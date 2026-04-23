# 04-preprocess: index

Full C99 (S)6.10 preprocessor: macros (hide-set), conditionals, #include, predefined macros.

## Upstream deps

- 03-lex

## Tasks (pick in order)

- [x] [01-tokenize-lines](01-tokenize-lines.md)
- [x] [02-directive-parser](02-directive-parser.md)
- [x] [03-include-search-path](03-include-search-path.md)
- [x] [04-include-guard-detection](04-include-guard-detection.md)
- [ ] [05-pragma-once](05-pragma-once.md)
- [ ] [06-define-object-like](06-define-object-like.md)
- [ ] [07-define-function-like](07-define-function-like.md)
- [ ] [08-hide-set-expansion](08-hide-set-expansion.md)
- [ ] [09-stringize-hash](09-stringize-hash.md)
- [ ] [10-paste-hashhash](10-paste-hashhash.md)
- [ ] [11-variadic-va-args](11-variadic-va-args.md)
- [ ] [12-predefined-macros](12-predefined-macros.md)
- [ ] [13-if-expression-const-eval](13-if-expression-const-eval.md)
- [ ] [14-conditional-stack](14-conditional-stack.md)
- [ ] [15-line-directive](15-line-directive.md)
- [ ] [16-error-pragma](16-error-pragma.md)
- [ ] [17-unit-tests](17-unit-tests.md)
- [ ] [18-chibicc-preprocess-tests](18-chibicc-preprocess-tests.md)
- [ ] [19-fuzz-target](19-fuzz-target.md)

## Downstream

- 05-parse, 11-conformance (chibicc preprocess)
