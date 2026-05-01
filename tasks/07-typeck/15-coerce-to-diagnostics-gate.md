> ✓ done — 2026-05-01

# 07-15: Make coercion failures diagnostic, not silent

**Phase:** 07-typeck    **Depends on:** 07-14    **Milestone:** M3 pre-codegen stabilization

## Goal
Eliminate silent fallthrough in assignment-compatible coercion. If an
initializer, assignment, call argument, or return value cannot be
converted to the required type, typeck must emit a diagnostic and leave
an explicit error state instead of handing mismatched types to CFG and
LLVM.

## Scope
- In: make the coercion helper return a structured result:
  converted expr id, no-op expr id, or typed error.
- In: route pointer conversion failures to E0081/E0082-style diagnostics
  with source labels.
- In: handle arithmetic narrowing warning policy explicitly, preserving
  current W0008 behavior.
- In: ensure downstream callers can avoid inserting bogus `Convert`
  nodes after an error.
- Out: full C implicit-int legacy extension support.

## Deliverables
- Refactored coercion API.
- Updated local initializer, assignment, call argument, and return
  call sites.
- Tests that prove invalid pointer/object conversions produce
  diagnostics and do not silently reach CFG.

## Acceptance
- `char *p; int *q; p = q;` emits an incompatible pointer diagnostic.
- `int *p = 42;` emits an integer-pointer diagnostic unless the value is
  a null pointer constant.
- The driver stops before CFG when such errors are emitted.
- No `coerce_to` call site ignores a conversion error.

## References
- C99 §6.5.16.1.
- Existing `AssignError` / `ConvertError`.
- 07-05, 07-06, 07-07.
