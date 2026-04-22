# 04-preprocess

**Goal of the phase.** Replace `rcc_preprocess::Preprocessor::run`'s
pass-through stub with the full C99 §6.10 preprocessor: directive
parsing, `#include` resolution, macro expansion with Prosser hide-set,
conditional compilation, predefined macros.

## Tasks

| # | File | Summary |
|---|------|---------|
| 01 | [`01-tokenize-lines.md`](01-tokenize-lines.md) | Chunk pp-tokens by logical line (directive boundary). |
| 02 | [`02-directive-parser.md`](02-directive-parser.md) | Recognise `#` + directive name → `Directive`. |
| 03 | [`03-include-search-path.md`](03-include-search-path.md) | Resolve `"..."` and `<...>` forms. |
| 04 | [`04-include-guard-detection.md`](04-include-guard-detection.md) | Detect `#ifndef X / #define X / ... / #endif`. |
| 05 | [`05-pragma-once.md`](05-pragma-once.md) | `#pragma once` cache. |
| 06 | [`06-define-object-like.md`](06-define-object-like.md) | `#define NAME tok...`. |
| 07 | [`07-define-function-like.md`](07-define-function-like.md) | `#define NAME(p1,p2,...) ...`. |
| 08 | [`08-hide-set-expansion.md`](08-hide-set-expansion.md) | Prosser algorithm. |
| 09 | [`09-stringize-hash.md`](09-stringize-hash.md) | `#x` operator. |
| 10 | [`10-paste-hashhash.md`](10-paste-hashhash.md) | `a ## b` operator. |
| 11 | [`11-variadic-va-args.md`](11-variadic-va-args.md) | `__VA_ARGS__`. |
| 12 | [`12-predefined-macros.md`](12-predefined-macros.md) | `__FILE__`, `__LINE__`, `__STDC__`, ... |
| 13 | [`13-if-expression-const-eval.md`](13-if-expression-const-eval.md) | `#if` expression evaluator. |
| 14 | [`14-conditional-stack.md`](14-conditional-stack.md) | `#if/#ifdef/#elif/#else/#endif` state machine. |
| 15 | [`15-line-directive.md`](15-line-directive.md) | `#line N "file"`. |
| 16 | [`16-error-pragma.md`](16-error-pragma.md) | `#error`, other `#pragma`s. |
| 17 | [`17-unit-tests.md`](17-unit-tests.md) | Table-driven expansion tests. |
| 18 | [`18-chibicc-preprocess-tests.md`](18-chibicc-preprocess-tests.md) | Run chibicc's macro tests. |
| 19 | [`19-fuzz-target.md`](19-fuzz-target.md) | Fuzz the preprocessor driver. |

## Exit criteria

- `chibicc/test/macro.c` compiles when the rest of the compiler is
  ready (upstream dependency).
- `rcc --emit=pp file.c` produces output byte-identical (modulo
  whitespace-insensitive comparison) to `cc -E file.c` on a corpus
  of small programs.
