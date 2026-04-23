# 05-parse: index

Turn the pp-token stream into a complete C99 AST. Phase-7 conversion + recursive-descent + Pratt + typedef-name hack.

## Upstream deps

- 03-lex, 04-preprocess

## Tasks (pick in order)

- [x] [01-phase7-token-conversion](01-phase7-token-conversion.md)
- [x] [02-keyword-classification](02-keyword-classification.md)
- [x] [03-integer-literal-decoder](03-integer-literal-decoder.md)
- [x] [04-float-literal-decoder](04-float-literal-decoder.md)
- [x] [05-char-escape-sequences](05-char-escape-sequences.md)
- [ ] [06-adjacent-string-concat](06-adjacent-string-concat.md)
- [ ] [07-primary-expressions](07-primary-expressions.md)
- [ ] [08-pratt-precedence](08-pratt-precedence.md)
- [ ] [09-postfix-unary](09-postfix-unary.md)
- [ ] [10-cast-sizeof](10-cast-sizeof.md)
- [ ] [11-conditional-expr](11-conditional-expr.md)
- [ ] [12-comma-expr](12-comma-expr.md)
- [ ] [13-statements-expr-block](13-statements-expr-block.md)
- [ ] [14-statements-if-else](14-statements-if-else.md)
- [ ] [15-statements-loops](15-statements-loops.md)
- [ ] [16-statements-switch-case](16-statements-switch-case.md)
- [ ] [17-statements-jumps](17-statements-jumps.md)
- [ ] [18-declarations-decl-specs](18-declarations-decl-specs.md)
- [ ] [19-declarator-tree](19-declarator-tree.md)
- [ ] [20-abstract-declarator](20-abstract-declarator.md)
- [ ] [21-typedef-name-hack](21-typedef-name-hack.md)
- [ ] [22-struct-union-fields](22-struct-union-fields.md)
- [ ] [23-enum-enumerators](23-enum-enumerators.md)
- [ ] [24-init-list-designators](24-init-list-designators.md)
- [ ] [25-function-definition](25-function-definition.md)
- [ ] [26-kr-declarations](26-kr-declarations.md)
- [ ] [27-error-recovery](27-error-recovery.md)
- [ ] [28-unit-tests-grammar](28-unit-tests-grammar.md)
- [ ] [29-ui-tests](29-ui-tests.md)
- [ ] [30-ctestsuite-parse-smoke](30-ctestsuite-parse-smoke.md)

## Downstream

- 06-hir-lower
