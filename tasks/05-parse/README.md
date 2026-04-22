# 05-parse

**Goal of the phase.** Stand the recursive-descent + Pratt parser up so
`rcc_parse::parse` returns a fully populated `TranslationUnit` for
every well-formed C99 source file. Includes phase-7 conversion
(pp-tokens → tokens: keyword classification, literal decoding, string
concatenation) and the "typedef-name hack".

## Tasks

| # | File | Summary |
|---|------|---------|
| 01 | [`01-phase7-token-conversion.md`](01-phase7-token-conversion.md) | Pp-token → C token driver. |
| 02 | [`02-keyword-classification.md`](02-keyword-classification.md) | Identifier → `Keyword` or `Ident`. |
| 03 | [`03-integer-literal-decoder.md`](03-integer-literal-decoder.md) | pp-number → `IntLiteral`. |
| 04 | [`04-float-literal-decoder.md`](04-float-literal-decoder.md) | pp-number → `FloatLiteral`. |
| 05 | [`05-char-escape-sequences.md`](05-char-escape-sequences.md) | `'\\n'`, `'\\x41'`, `'\\u00e9'`. |
| 06 | [`06-adjacent-string-concat.md`](06-adjacent-string-concat.md) | `"a" "b"` → `"ab"`. |
| 07 | [`07-primary-expressions.md`](07-primary-expressions.md) | ident, literal, paren. |
| 08 | [`08-pratt-precedence.md`](08-pratt-precedence.md) | Binary-op Pratt table. |
| 09 | [`09-postfix-unary.md`](09-postfix-unary.md) | `a[b]`, `a.b`, `a->b`, `a++`, `*a`, `&a`. |
| 10 | [`10-cast-sizeof.md`](10-cast-sizeof.md) | `(T)e`, `sizeof e`, `sizeof(T)`. |
| 11 | [`11-conditional-expr.md`](11-conditional-expr.md) | `a ? b : c`. |
| 12 | [`12-comma-expr.md`](12-comma-expr.md) | `a, b`. |
| 13 | [`13-statements-expr-block.md`](13-statements-expr-block.md) | `;`, `{ ... }`. |
| 14 | [`14-statements-if-else.md`](14-statements-if-else.md) | `if / else`. |
| 15 | [`15-statements-loops.md`](15-statements-loops.md) | `while / do-while / for`. |
| 16 | [`16-statements-switch-case.md`](16-statements-switch-case.md) | `switch`, `case`, `default`. |
| 17 | [`17-statements-jumps.md`](17-statements-jumps.md) | `break`, `continue`, `return`, `goto`. |
| 18 | [`18-declarations-decl-specs.md`](18-declarations-decl-specs.md) | storage + type spec + quals. |
| 19 | [`19-declarator-tree.md`](19-declarator-tree.md) | pointer/array/function chain. |
| 20 | [`20-abstract-declarator.md`](20-abstract-declarator.md) | for type names, params. |
| 21 | [`21-typedef-name-hack.md`](21-typedef-name-hack.md) | Parser ↔ scope feedback. |
| 22 | [`22-struct-union-fields.md`](22-struct-union-fields.md) | `struct { ... };`. |
| 23 | [`23-enum-enumerators.md`](23-enum-enumerators.md) | `enum { ... };`. |
| 24 | [`24-init-list-designators.md`](24-init-list-designators.md) | `{ .x = 1, [2] = 3 }`. |
| 25 | [`25-function-definition.md`](25-function-definition.md) | top-level `T f(args) { ... }`. |
| 26 | [`26-kr-declarations.md`](26-kr-declarations.md) | K&R old-style. |
| 27 | [`27-error-recovery.md`](27-error-recovery.md) | Sync-token resync. |
| 28 | [`28-unit-tests-grammar.md`](28-unit-tests-grammar.md) | One test per production. |
| 29 | [`29-ui-tests.md`](29-ui-tests.md) | Bad input → stable stderr. |
| 30 | [`30-ctestsuite-parse-smoke.md`](30-ctestsuite-parse-smoke.md) | Parse every c-testsuite file. |

## Exit criteria

- `rcc_parse::parse` returns `Some(TranslationUnit)` for every well-
  formed c-testsuite input.
- `cargo test -p rcc_parse`: ≥ 80 % line coverage.
- Bad inputs produce stable, reviewed `.stderr` fixtures.
