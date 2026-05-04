# 07-21: aggregate rvalue member access

**Phase:** 07-typeck    **Depends on:** 07-13    **Milestone:** M6+

## Goal
Type member access on aggregate rvalues correctly, instead of assuming every
`a.b` base can be lowered as an lvalue place.

## Trigger
- `c-testsuite::00204` panics in CFG lowering at `lower_as_place` while
  compiling member access on returned aggregate values such as
  `fr_hfa12().a`.

## Scope
- In:
  - When the base of `.` is an aggregate rvalue, make the field expression an
    rvalue of the field type.
  - Preserve lvalue behavior when the base is an lvalue aggregate.
  - Ensure CFG lowering has an explicit rvalue extraction path rather than
    calling `lower_as_place` on a call result.
- Out:
  - Full aggregate ABI correctness for every struct size/class.

## Deliverables
- Typeck tests for `make_struct().field`.
- CFG/codegen regression that extracts a field from a returned struct.
- c-testsuite `00204` rerun to find the next blocker after the panic is gone.

## Acceptance
- `rcc` never panics on aggregate-rvalue member access.
- `struct S f(void); return f().x;` executes correctly for scalar fields.

## References
- C99 §6.5.2.3
- `third_party/testsuites/c-testsuite/tests/single-exec/00204.c`
