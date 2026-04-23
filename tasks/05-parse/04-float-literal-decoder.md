> ✓ done — 2026-04-23

# 05-04: Float literal decoder

**Phase:** 05-parse    **Depends on:** 05-01    **Milestone:** M2

## Goal
Decode `PpNumberKind::Float` into `FloatLiteral { value, suffix }`.
Support decimal floats and hex floats (`0x1.8p1` etc.; C99 §6.4.4.2).

## Scope
- In: parse with Rust's `f64::from_str` for decimal; hand-rolled for
  hex floats; suffix `f`/`F` → `FloatSuffix::F`, `l`/`L` → `L`, none
  → default `double`.
- Out: `long double` value fidelity — we store as `f64` and note the
  lossy cast in a comment; full `f128` arithmetic is out of scope.

## Deliverables
- `decode_float(text: &str) -> Result<FloatLiteral, Diagnostic>`.
- Tests: `1.0`, `.5e10`, `0x1.0p0`, `3.14f`, `2.0L`.

## Acceptance
- Overflow → `+∞` with diagnostic W0002 ("float literal overflow").
- Hex float `0x1.0p3` == `8.0`.

## References
- C99 §6.4.4.2.
