# 11-16a: tcc-tests2 float codegen

**Phase:** 11-conformance    **Depends on:** 11-16    **Milestone:** M6

## Goal
Fix the floating-point regressions exposed by tcc-tests2.

## Scope
- In: `tcc-tests2::22_floating_point` and
  `tcc-tests2::70_floating_point_literals`.
- Out: decimal floating types and non-C99 math-library surface.

## Deliverables
- Reduced regression tests that fail before the fix.
- Codegen/typeck changes needed for float operands, casts, comparisons,
  constants, and arithmetic to match C99 semantics.
- Removal of both cases from the failing tcc-tests2 bucket.

## Acceptance
- Both target cases pass in WSL through
  `rcc_conformance_run --suite tcc-tests2`.
- No internal codegen error remains for a legal C99 float operand.

## References
- `target/wsl/tcc-tests2-16-final.json`
- C99 §6.3.1.5, §6.5.
