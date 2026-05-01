# 09-06: Function and global declarations

**Phase:** 09-codegen-llvm    **Depends on:** 09-01, 09-05    **Milestone:** M3

## Goal

Declare every externally visible LLVM symbol before emitting bodies so calls,
global references, and initializers can resolve by `DefId`.

## Scope

- In: function prototypes/definitions, `static`, `extern`, C99 `inline`,
  `extern inline`, variadic flag, file-scope globals, and string literal symbols.
- Out: initializer payload materialization; owned by 09-11.

## Deliverables

- `DefId -> FunctionValue` and `DefId -> GlobalValue` maps.
- Tests for duplicate declaration reuse and linkage selection.

## Acceptance

- A call to a later-defined function resolves without creating a duplicate LLVM
  function.
- `static` file-scope definitions use internal linkage; ordinary declarations
  use external linkage.

## References

- C99 6.2.2
- C99 6.7.4
