# 07-18: Function call prototype and varargs constraints

> ✓ done — 2026-05-01

**Phase:** 07-typeck    **Depends on:** 07-17    **Milestone:** M6

## Goal
Ensure call expressions are fully type-checked before LLVM ABI
classification. Codegen needs argument types after prototype coercion
and default argument promotions; it should not infer C call semantics
from raw HIR.

## Scope
- In: verify callee has function or pointer-to-function type.
- In: check argument count for prototyped functions.
- In: coerce fixed arguments to parameter types using the structured
  coercion API from 07-15.
- In: apply default argument promotions for unprototyped and variadic
  trailing arguments.
- In: reject calls through incompatible non-function expressions.
- Out: builtins such as `__builtin_va_start`; owned by phase 15.

## Deliverables
- Function-call type checker with explicit prototype metadata.
- Tests for fixed prototypes, no-prototype declarations, variadic
  functions, too few/many arguments, and callee type errors.
- MIR/source pipeline fixture that proves CFG call operands are already
  converted.

## Acceptance
- `int f(long); int g(int x) { return f(x); }` inserts an argument
  conversion to `long`.
- `int f(int); f(1, 2);` emits an argument-count diagnostic.
- `int printf(char *, ...); printf("%d", (char)1);` promotes the
  variadic `char` argument to `int`.
- `int x; x();` emits a non-callable diagnostic.

## References
- C99 §6.5.2.2.
- 09-04 / 09-05 ABI classification tasks.
