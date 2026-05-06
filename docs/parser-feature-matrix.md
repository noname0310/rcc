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
| Initializer lists, C99 field/index designators, and GNU range designator syntax | complete | HIR/typeck flatten and type-check initializers; `06-24` owns range expansion |
| Function definitions, prototypes, variadic functions, K&R definitions | complete | HIR/typeck validate parameter types and obsolete-style semantics |
| GNU `__attribute__((...))` attachment syntax | complete | Phase 14 validates attachment rules and attribute semantics |
| GNU inline assembly statement syntax | complete | Phase 14 validates operand shape; LLVM codegen lowers to inline asm |
| Parser recovery after malformed declarations/statements | complete | Diagnostics own final wording |

## Parsed But Checked Later

| Syntax | Parser behavior | Later check |
|---|---|---|
| Assignment to non-unary / non-lvalue expressions | Accepts AST shape | HIR/typeck reject non-modifiable lvalues |
| `break` / `continue` outside a valid target | Accepts node | HIR validates target stacks |
| Duplicate labels, duplicate `case`, multiple `default` | Accepts syntax | HIR/typeck validates per function/switch |
| Invalid type combinations that require semantic context | Emits local specifier diagnostics where possible | HIR/typeck validates complete type constraints |
| K&R-style definitions | Parses and emits obsolete-style warning | HIR/typeck validate parameter declarations |
| GNU initializer range designators `[lo ... hi]` | Preserves a distinct range designator and warns in strict C99 mode | HIR lowering expands ranges and overlap semantics in `06-24` |
| GNU attributes `__attribute__((...))` | Preserves raw attribute names and argument tokens; warns in strict C99 mode | Phase 14 validates allowed sites, argument shape, and attribute effects |
| GNU inline asm `asm(...)` / `__asm__(...)` | Preserves template, qualifiers, operands, constraints, clobbers, and spans; warns in strict C99 mode | Phase 14 validates constraints; codegen emits LLVM inline asm |

## Parser Blockers Still Open

None for the C99 parser surface as of 05-40. The remaining
c-testsuite parse xfails are owned by preprocessor behavior,
freestanding headers/runtime, HIR/initializer lowering, or explicitly
outside-release extension semantics.

## External Suite Notes

`third_party/testsuites/c-testsuite/xfail.toml` must name the concrete
parser blocker when a parse-only failure is caused by syntax. Header,
preprocessor, HIR, typeck, runtime, and extension-semantic failures must
be labelled with their actual owner so agents do not fix the wrong
crate.
