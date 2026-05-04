# 11b: function name predefined identifiers

**Phase:** 11-conformance    **Depends on:** 11-11    **Milestone:** M4+

## Goal
Implement function-scope predefined function name strings required by
`function.c`: C99 `__func__` and GNU `__FUNCTION__`.

## Scope
- In:
  - Treat `__func__` as the C99 predefined identifier with type compatible with
    a function-scope `static const char[]` containing the current function name.
  - Support GNU `__FUNCTION__` as an alias behind an explicit GNU option or
    warning gate.
  - Preserve correct behavior in `sizeof(__func__)`, pointer decay, returns,
    calls to `strcmp`, and nested scopes inside the same function.
  - Add strict-mode diagnostics or warnings for `__FUNCTION__` without GNU
    compatibility enabled.
- Out:
  - Pretty-function signatures such as GNU `__PRETTY_FUNCTION__`.

## Deliverables
- Parser/HIR/typeck/CFG/codegen support or an equivalent lower-stage
  representation for predefined function name arrays.
- Unit tests for `main`, a non-main function, `sizeof(__func__)`, and
  `__FUNCTION__`.
- An E2E fixture that returns or compares the expected function name.

## Acceptance
- `char *f(void) { return __func__; }` returns `"f"`.
- `sizeof(__func__)` includes the trailing NUL.
- `__FUNCTION__` works under GNU compatibility and is diagnosed or warned in
  strict C99 mode.
- `function.c` advances past all `__func__` / `__FUNCTION__` undeclared
  diagnostics.

## References
- C99 §6.4.2.2 predefined identifiers.
- chibicc `test/function.c` lines 118, 122, 289, 290, 292.
