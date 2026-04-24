# 14-02: `__has_include` in preprocessor conditionals

**Phase:** 14-lang-extensions    **Depends on:** 14-01    **Milestone:** M5

## Goal
Support `__has_include(<header>)` and `__has_include("header")` inside
`#if` / `#elif` expressions. Evaluate to `1` if the named header is
locatable via the current include search path, `0` otherwise.

## Scope
- In: recognize `__has_include` as a special macro in `#if`
  expression evaluation, probe the include search path without
  actually opening the file.
- Out: `__has_include_next` (GNU extension, defer).

## Deliverables
- `__has_include` evaluator in `rcc_preprocess` conditional
  expression logic.
- Test: `#if __has_include(<stddef.h>)` evaluates to 1 when
  the header exists, 0 for a non-existent header.

## Acceptance
- `#if __has_include(<nonexistent_header.h>)` evaluates to 0.
- `#if __has_include("existing_local.h")` evaluates to 1 when the
  file is present in the include search path.

## References
- C23 §6.10.1 (adopted from `__has_include` extension); widely
  supported as extension in C99/C11 mode by GCC/Clang.
