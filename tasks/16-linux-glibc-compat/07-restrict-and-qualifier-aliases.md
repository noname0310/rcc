# 16-07: Restrict And Qualifier Aliases

> ✓ done — 2026-05-06

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-06-gnu-header-attribute-tolerance  
**Milestone:** hosted-linux

## Goal

Normalize glibc and GNU qualifier spellings into the compiler's existing C99
qualifier model.

## Scope

- In: `__restrict`, `__restrict__`, `__restrict_arr`, `__const`, and related
  aliases found in system headers.
- In: parser, HIR lowering, and type-check tests.
- Out: inventing outside-release qualifier semantics.

## Acceptance

- [x] The aliases are accepted only in the compatibility mode or when already
      enabled by GNU extension options.
- [x] Lowered types preserve restrict/const information where rcc models it.
- [x] Tests include pointer parameters and array parameters from glibc-like
      declarations.
- [x] Strict C99 behavior remains unchanged.
