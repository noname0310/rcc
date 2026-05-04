# 09-27: enum bitfield unsignedness policy

> ✓ done — 2026-05-04

**Phase:** 09-codegen-llvm    **Depends on:** 09-21    **Milestone:** M6+

## Goal
Make enum-typed bitfield loads/stores match the project policy for positive
enum values that do not fit in a signed bitfield of the selected width.

## Trigger
- `c-testsuite::00218` stores `AMBIG_CONV` into an 8-bit enum bitfield and
  expects the value to be zero-extended, not sign-extended.

## Scope
- In:
  - Decide and document the implementation-defined policy for enum bitfields.
  - Teach layout/codegen whether an enum bitfield should be treated as signed
    or unsigned for load extension.
  - Keep ordinary signed integer bitfields sign-extending.
- Out:
  - Nonstandard bitfield packing beyond the current target layout policy.

## Deliverables
- Layout/codegen tests for signed int, unsigned int, and enum bitfields.
- c-testsuite regression for `00218`.
- Documentation note in the relevant layout/codegen docs or task file.

## Acceptance
- `c-testsuite::00218` produces no stdout and exits successfully.
- Existing bitfield tests remain green.

## Implementation note
- rcc keeps ordinary enum object signedness tied to the resolved enum
  representation, but enum-typed bitfield loads are zero-extended as an
  implementation-defined policy. Signed integer bitfields continue to
  sign-extend.

## References
- C99 §6.7.2.1 implementation-defined bitfield signedness
- `third_party/testsuites/c-testsuite/tests/single-exec/00218.c`
