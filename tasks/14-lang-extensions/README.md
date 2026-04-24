# 14-lang-extensions

**Goal of the phase.** Add preprocessor and parser extensions that
real-world C code depends on: `_Pragma`, `__has_include`,
`__COUNTER__`, `__attribute__` syntax and semantics, inline
assembly, dependency file generation, and target-dependent
predefined macros.

## Tasks

| # | File | Summary |
|---|------|---------|
| 01 | [`01-pragma-operator.md`](01-pragma-operator.md) | `_Pragma("once")` C99 §6.10.9 operator. |
| 02 | [`02-has-include.md`](02-has-include.md) | `__has_include` in `#if` expressions. |
| 03 | [`03-counter-macro.md`](03-counter-macro.md) | `__COUNTER__` predefined macro. |
| 04 | [`04-cli-undefine.md`](04-cli-undefine.md) | `-U NAME` CLI flag. |
| 05 | [`05-dependency-generation.md`](05-dependency-generation.md) | `-M`/`-MM`/`-MF`/`-MT` depfile output. |
| 06 | [`06-target-predefined-macros.md`](06-target-predefined-macros.md) | `__x86_64__`, `__linux__`, `__SIZEOF_*__`, etc. |
| 07 | [`07-attribute-syntax.md`](07-attribute-syntax.md) | Parse `__attribute__((...))` syntax. |
| 08 | [`08-attribute-common.md`](08-attribute-common.md) | Semantics for packed, aligned, noreturn, etc. |
| 09 | [`09-inline-asm-syntax.md`](09-inline-asm-syntax.md) | Parse GCC `__asm__` syntax. |
| 10 | [`10-inline-asm-codegen.md`](10-inline-asm-codegen.md) | Lower inline asm to LLVM. |

## Exit criteria

- `_Pragma("once")` prevents double inclusion.
- `__has_include(<stddef.h>)` evaluates correctly in `#if`.
- `__attribute__((packed))` produces a struct with no padding.
- A simple `__asm__ volatile ("nop")` compiles to valid LLVM IR.
