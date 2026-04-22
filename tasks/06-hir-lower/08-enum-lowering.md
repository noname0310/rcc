# 06-08: Enum lowering

**Phase:** 06-hir-lower    **Depends on:** 06-03    **Milestone:** M4

## Goal
Evaluate each enumerator value (default = previous + 1; first = 0)
and register the enumerator as an `ordinary` name with value
`DefKind::Global` ... no, per C99 §6.4.4.3 enumerators are a separate
sub-kind, but they live in the ordinary namespace.

## Scope
- In: fold `const-expr` using `rcc_typeck::ConstEval`; store the i128
  value; pick a representable integer type (§6.7.2.2p4); insert into
  `Resolver::ordinary` as enumerator entries.
- Out: the underlying integer type choice *rule* is simplified to
  `int` in M4; promoted to §6.7.2.2p4 algorithm in M6.

## Deliverables
- `lower_enum(spec) -> DefKind::Enum`.
- Tests: default-only, explicit values, out-of-int-range (W0006).

## Acceptance
- `enum { A, B = 5, C }`: A=0, B=5, C=6.
- Duplicate enumerator name in same scope → E0078.

## References
- C99 §6.7.2.2.
