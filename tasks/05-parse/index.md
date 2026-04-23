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
- [x] [06-adjacent-string-concat](06-adjacent-string-concat.md)
- [x] [07-primary-expressions](07-primary-expressions.md)
- [x] [08-pratt-precedence](08-pratt-precedence.md)
- [x] [09-postfix-unary](09-postfix-unary.md)
- [x] [10-cast-sizeof](10-cast-sizeof.md)
- [x] [10b-compound-literal](10b-compound-literal.md)
- [x] [11-conditional-expr](11-conditional-expr.md)
- [x] [12-comma-expr](12-comma-expr.md)
- [x] [13-statements-expr-block](13-statements-expr-block.md)
- [x] [14-statements-if-else](14-statements-if-else.md)
- [x] [15-statements-loops](15-statements-loops.md)
- [x] [16-statements-switch-case](16-statements-switch-case.md)
- [x] [17-statements-jumps](17-statements-jumps.md)
- [x] [18-declarations-decl-specs](18-declarations-decl-specs.md)
- [x] [19-declarator-tree](19-declarator-tree.md)
- [x] [20-abstract-declarator](20-abstract-declarator.md)
- [x] [21-typedef-name-hack](21-typedef-name-hack.md)
- [x] [22-struct-union-fields](22-struct-union-fields.md)
- [x] [23-enum-enumerators](23-enum-enumerators.md)
- [x] [24-init-list-designators](24-init-list-designators.md)
- [x] [25-function-definition](25-function-definition.md)
- [x] [26-kr-declarations](26-kr-declarations.md)
- [x] [27-error-recovery](27-error-recovery.md)
- [x] [28-unit-tests-grammar](28-unit-tests-grammar.md)
- [x] [29-ui-tests](29-ui-tests.md)
- [x] [30-ctestsuite-parse-smoke](30-ctestsuite-parse-smoke.md)

## Downstream

- 06-hir-lower
