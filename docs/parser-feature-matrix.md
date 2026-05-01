# Parser Feature Matrix

This document is the parser-facing source of truth for syntax coverage.
It separates syntax that `rcc_parse` owns from constraints intentionally
checked by HIR lowering, type checking, or later extension/runtime
phases.

## C99 Syntax Parsed

| Area | Status | Downstream owner |
|---|---:|---|
| Phase-7 token conversion, keyword classification, literal decoding, adjacent string concatenation | complete | HIR/typeck consume AST literal nodes |
| Decoded integer, float, character, and string literal payloads in AST | complete | HIR lowering consumes payload fields directly |
| Primary, postfix, unary, cast, `sizeof`, compound literal, binary, conditional, assignment, comma expressions | complete | HIR/typeck validate lvalues, conversions, and constant-expression rules |
| Labels, compound blocks, expression statements, `if`, `switch`, loops, jumps | complete | HIR validates labels, jump targets, switch constraints |
| Block declarations and `for` declaration init | complete | HIR preserves lexical scope and storage |
| Declaration specifiers, declarators, abstract declarators, typedef-name feedback | complete | HIR/typeck derive canonical types |
| Strict `type-name` parsing for cast, `sizeof(type)`, compound literals, and future builtin type args | complete | HIR/typeck validate semantic type constraints |
| GCC/Clang builtin type-argument syntax (`__builtin_offsetof`, `__builtin_types_compatible_p`) | complete | Phase 15 lowers layout and type-compatibility semantics |
| GNU statement expressions `({ ... })` | complete | HIR/CFG validate result type, lifetime, and label/codegen semantics |
| Struct/union fields, bit-fields, enum enumerators | complete | HIR/typeck evaluate layout, duplicate names, enum values |
| Initializer lists and C99 field/index designators | complete | HIR/typeck flatten and type-check initializers |
| Function definitions, prototypes, variadic functions, K&R definitions | complete | HIR/typeck validate parameter types and obsolete-style semantics |
| Parser recovery after malformed declarations/statements | complete | Diagnostics own final wording |

## Parsed But Checked Later

| Syntax | Parser behavior | Later check |
|---|---|---|
| Assignment to non-unary / non-lvalue expressions | Accepts AST shape | HIR/typeck reject non-modifiable lvalues |
| `break` / `continue` outside a valid target | Accepts node | HIR validates target stacks |
| Duplicate labels, duplicate `case`, multiple `default` | Accepts syntax | HIR/typeck validates per function/switch |
| Invalid type combinations that require semantic context | Emits local specifier diagnostics where possible | HIR/typeck validates complete type constraints |
| K&R-style definitions | Parses and emits obsolete-style warning | HIR/typeck validate parameter declarations |

## Parser Blockers Still Open

| Task | Syntax | Why it blocks later work |
|---|---|---|
| 05-37 | GNU range designators `[lo ... hi]` | c-testsuite `00216` needs range initializer syntax before HIR can expand it |
| 05-38 | GCC `__attribute__((...))` parser surface | Phase 14 attribute semantics need stable AST attachment sites |
| 05-39 | GCC inline asm parser surface | Inline asm codegen needs parsed templates, constraints, and clobbers |
| 05-40 | C11 `_Generic` | C11 compatibility tests need generic-selection syntax before type matching |
| 05-41 | Parser-owned xfail shrink | Remaining parse xfails must be reclassified after the blocker tasks land |

## External Suite Notes

`third_party/testsuites/c-testsuite/xfail.toml` must name the concrete
parser blocker when a parse-only failure is caused by syntax. Header,
preprocessor, HIR, typeck, runtime, and extension-semantic failures must
be labelled with their actual owner so agents do not fix the wrong
crate.
