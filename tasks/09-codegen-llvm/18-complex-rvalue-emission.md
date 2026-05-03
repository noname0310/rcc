# 09-18: Complex rvalue emission

> ✓ done — 2026-05-03

**Phase:** 09-codegen-llvm    **Depends on:** 09-05, 09-09, 09-14, 09-15    **Milestone:** M7

## Goal

Emit C99 `_Complex` values consistently across type lowering, conversions,
loads/stores, and arithmetic required by typed CFG.

## Scope

- In: representation choice, `ComplexFromReal`, `RealFromComplex`, complex load
  and store, zero imaginary construction, and supported complex arithmetic.
- Out: C11 imaginary types.

## Deliverables

- Complex helper module with extract/insert helpers.
- Tests for real-to-complex, complex-to-real, assignment, and simple arithmetic.

## Acceptance

- Backend honors the CFG contract that real extraction reads only the real
  component and discards the imaginary component.
- Complex values can pass through locals without layout/type mismatch.

## References

- C99 6.2.5
- `rcc_cfg::Rvalue::ComplexFromReal`
