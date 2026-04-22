# 04-07: Function-like `#define`

**Phase:** 04-preprocess    **Depends on:** 04-06    **Milestone:** M5

## Goal
Parse `#define NAME(p1,p2,...) body` with optional trailing `...` for
variadic. Store in `MacroDef { kind: FunctionLike { params, variadic }, ... }`.

## Scope
- In: parameter list parsing (comma-separated identifiers + optional
  `...`); no space allowed between `NAME` and `(` (C99 §6.10.3p10);
  detect duplicate parameter name → E0023.
- Out: invocation parsing (task 08).

## Deliverables
- Parser for parameter list.
- Tests: no params, multiple params, variadic, bad (`FOO ( a, a )`).

## Acceptance
- `#define MAX(a,b) ((a)>(b)?(a):(b))` stores with exactly 2 params.
- `#define V(...) __VA_ARGS__` sets `variadic = true`.

## References
- C99 §6.10.3.
