> ✓ done — 2026-04-23

# 04-11: Variadic `__VA_ARGS__`

**Phase:** 04-preprocess    **Depends on:** 04-07, 04-08, 04-10    **Milestone:** M5

## Goal
Handle function-like macros ending in `...`. Inside the body,
`__VA_ARGS__` expands to the comma-separated trailing arguments.
GNU-style `, ## __VA_ARGS__` elision (delete preceding comma when
empty) is a popular extension — ship it behind a feature gate off by
default.

## Scope
- In: argument collection recognises variadic; `__VA_ARGS__` as a
  pseudo-parameter in `expand`; diagnostic E0026 if used outside a
  variadic body.
- Out: C11's `__VA_OPT__` (not in C99; add as future work note).

## Deliverables
- `VA_ARGS_SYMBOL` const + lookup.
- Tests from chibicc's `test/macro.c` variadic section.

## Acceptance
- `#define LOG(fmt, ...) printf(fmt, __VA_ARGS__) / LOG("a",1,2)`
  expands to `printf("a", 1, 2)`.
- `LOG("a")` with zero extra args: emits diagnostic *unless* GNU ext
  is enabled, then elides the trailing comma.

## References
- C99 §6.10.3.1p2.
