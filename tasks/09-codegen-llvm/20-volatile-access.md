> ✓ done — 2026-05-03

# 09-20: Volatile access codegen

**Phase:** 09-codegen-llvm    **Depends on:** 09-09, 09-12    **Milestone:** M6

## Goal

Preserve C `volatile` semantics in LLVM load/store instructions and avoid
optimizing away required memory accesses.

## Scope

- In: volatile-qualified lvalue loads/stores, compound assignment access shape,
  increment/decrement, and volatile aggregate copy policy.
- Out: atomics (`_Atomic`) and C11 memory model.

## Deliverables

- Qualifier-aware load/store API.
- Tests that IR contains `load volatile` and `store volatile` where required.

## Acceptance

- Reads from volatile objects are emitted even when the result is unused.
- Non-volatile accesses remain non-volatile.

## References

- C99 6.7.3
- LLVM LangRef: volatile memory access
