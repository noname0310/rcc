# 11-16j: tcc-tests2 binary floating literals

**Phase:** 11-conformance    **Depends on:** 11-16a    **Milestone:** M7

## Goal
Decide whether rcc compatibility mode should implement TinyCC binary floating
constants.

## Scope
- In: the `#ifdef __TINYC__` block in
  `tcc-tests2::70_floating_point_literals`.
- Out: C99 decimal and hexadecimal floating constants; they are already covered
  by 11-16a.

## Deliverables
- A policy note: either keep the fixture xfailed because the construct is
  TinyCC-only, or add an explicit compatibility flag that defines the required
  macro and accepts binary floating constants.
- Parser/literal decoder tests if the feature is accepted.

## Acceptance
- The xfail entry for `70_floating_point_literals` is either removed after the
  feature passes, or retained with an explicit policy reference.
- C99 floating-literal tests continue to pass.

## References
- `third_party/testsuites/tcc-tests2/tests/tests2/70_floating_point_literals.c`
