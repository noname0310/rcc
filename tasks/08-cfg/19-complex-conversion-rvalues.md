# 08-19: Complex conversion rvalues

**Phase:** 08-cfg    **Depends on:** 08-18    **Milestone:** M6 stabilization

## Goal
Preserve real/complex conversion semantics in CFG instead of lowering
them as no-op casts. Codegen should consume explicit CFG semantics, not
reconstruct complex conversion intent from lost HIR context.

## Scope
- In: add CFG representation for real-to-complex and complex-to-real
  conversions.
- In: model construction of the imaginary zero part for real-to-complex.
- In: model extraction of the real component for complex-to-real.
- In: tests for assignment, return, call argument, and conditional
  expression conversions involving `_Complex`.
- Out: optimized complex arithmetic lowering; this task only preserves
  conversion meaning.

## Deliverables
- Either new `CastKind` variants or dedicated `Rvalue` variants for
  complex component operations.
- `lower_conversion` no longer maps `RealToComplex` or `ComplexToReal`
  to `None`.
- MIR snapshot fixtures showing explicit complex conversion nodes.
- A short backend contract note in this task or code comments so phase
  09 knows how to emit these nodes.

## Acceptance
- `rg "RealToComplex \\| ComplexToReal => None" crates/rcc_cfg/src`
  returns no match.
- `double _Complex f(double x) { return x; }` shows an explicit complex
  construction in MIR.
- `double f(double _Complex z) { return z; }` shows an explicit real
  extraction in MIR.

## References
- C99 §6.3.1.7 real and complex conversions.
- `rcc_hir::ConvertKind`.
- `rcc_cfg::CastKind` and `rcc_cfg::Rvalue`.
