> ✓ done — 2026-05-03

# 09-17: VLA stack allocation and length values

**Phase:** 09-codegen-llvm    **Depends on:** 09-04, 09-09, 09-10    **Milestone:** M5

## Goal

Implement runtime allocation and length handling for C99 variable length arrays
as represented by CFG `LocalDecl.vla_len` and `Rvalue::Len`.

## Scope

- In: dynamic `alloca`, saved runtime element count, `sizeof(VLA)`, nested VLA
  locals, scope exit behavior, and VLA projection addressing.
- Out: non-stack VLA extensions.

## Deliverables

- VLA local materialization path distinct from entry-block allocas.
- Tests for `int a[n]; sizeof a; a[i] = ...;`.

## Acceptance

- VLA stack allocation happens at runtime at the lexical declaration point.
- `Rvalue::Len(place)` returns the saved runtime length, not a static sentinel.

## References

- C99 6.7.5.2
- `rcc_cfg::LocalDecl::vla_len`
