# 09-11: Global initializer materialization

**Phase:** 09-codegen-llvm    **Depends on:** 09-05, 09-06    **Milestone:** M4

## Goal

Lower HIR `GlobalInit` and string literals into LLVM constants and globals,
including aggregate designator paths and zero-fill.

## Scope

- In: `GlobalInitValue::{Int, Float, Address, StringLiteral, Zero}`, nested
  array/record constants, union first-active-field policy, string literal
  interning, and relocatable global addresses.
- In: reject `GlobalInitValue::Error` before emitting invalid IR.
- Out: TLS (`_Thread_local`) and C11.

## Deliverables

- `GlobalCx` helper for string/global interning and initializer construction.
- Fixtures for scalar, array, struct, union, nested designator, and string init.

## Acceptance

- `static int x = 5;` emits `@x = internal global i32 5`.
- Identical string literals share a global when semantics allow it.
- Malformed initializer leaves cannot reach LLVM as dummy constants.

## References

- LLVM LangRef: Global variables
- `rcc_hir::GlobalInit`
