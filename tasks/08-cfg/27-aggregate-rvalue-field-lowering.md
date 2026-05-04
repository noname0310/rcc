> ✓ done — 2026-05-04

# 08-27: aggregate rvalue field lowering

**Phase:** 08-cfg    **Depends on:** 07-21, 08-10    **Milestone:** M6+

## Goal
Lower field extraction from aggregate rvalues without routing through
`lower_as_place`.

## Trigger
- `c-testsuite::00204` currently panics because `Field { base: Call(...), .. }`
  is handled as if the base were an lvalue place.

## Scope
- In:
  - Materialize aggregate rvalues into a temporary when needed.
  - Extract the requested field as an rvalue.
  - Keep lvalue aggregate field access using normal place projections.
- Out:
  - Reworking the full SysV aggregate ABI; if ABI mismatches remain after this
    task, create a separate 09-codegen task.

## Deliverables
- CFG unit tests for `make_struct().x`.
- LLVM E2E regression for a small returned struct.

## Acceptance
- No `lower_as_place` panic for aggregate rvalue fields.
- The first `00204` blocker is replaced by either pass or a more specific ABI
  failure.

## Result
- Implemented in commit `9a2766e` while completing the upstream 07-21 task,
  because the fix required typeck category validation and CFG lowering together.
- `struct S f(void); return f().x;` compiles and executes for scalar fields.
- `c-testsuite::00204` now advances to the 09-codegen aggregate ABI blocker:
  `unsupported direct ABI class`.

## References
- `crates/rcc_cfg/src/lower.rs`
- `tasks/07-typeck/21-aggregate-rvalue-member-access.md`
