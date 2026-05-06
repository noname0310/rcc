# 15a-02: C11 Keyword Tokenization

**Phase:** 15a-c11-transition  
**Depends on:** 15a-01-language-standard-mode  
**Milestone:** c11-transition

## Goal

Teach phase-7 token classification about the C11 keywords while preserving
C99-mode diagnostics for code that uses those spellings as extensions.

## Scope

- In: `_Alignas`, `_Alignof`, `_Atomic`, `_Generic`, `_Noreturn`,
  `_Static_assert`, and `_Thread_local`.
- In: C11-mode keyword classification tests.
- In: C99-mode policy tests: either reject these spellings as identifiers in
  specifier/operator positions or accept with a compatibility warning, but make
  the behavior explicit.
- In: ensure `_Complex` and `_Imaginary` remain C99 behavior.
- Out: C23 keywords such as `_BitInt`, `typeof_unqual`, `nullptr`,
  `constexpr`, or `_Decimal*`.

## Acceptance

- [ ] Every C11 keyword round-trips through `classify_ident` / phase-7 tests.
- [ ] C99 mode has documented behavior for each C11 keyword.
- [ ] No existing typedef-name disambiguation regression.
- [ ] The keyword table count test is updated to distinguish C99 core keywords
      from C11 keywords instead of using one flat number.

## References

- N1570 6.4.1 keywords.
- Clang's "C keywords supported in all language modes" list as an
  implementation comparison point.
