# 16-07: Restrict And Qualifier Aliases

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
- Out: inventing non-C99 qualifier semantics.

## Acceptance

- [ ] The aliases are accepted only in the compatibility mode or when already
      enabled by GNU extension options.
- [ ] Lowered types preserve restrict/const information where rcc models it.
- [ ] Tests include pointer parameters and array parameters from glibc-like
      declarations.
- [ ] Strict C99 behavior remains unchanged.
