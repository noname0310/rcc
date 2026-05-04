> ✓ done — 2026-05-04

# 06-31: aggregate initializer brace elision cursor

**Phase:** 06-hir-lower    **Depends on:** 06-21    **Milestone:** M6+

## Goal
Implement C99 aggregate initializer brace elision so a flat list can fill
nested arrays and structs in declaration order.

## Trigger
- `c-testsuite::00205` initializes an array of `PT`, where `PT` starts with
  `I c[4]`, using a flat list. `rcc` currently treats scalar leaves as
  incompatible with the containing aggregate and emits `E0081`.

## Scope
- In:
  - Track an aggregate cursor that descends into subaggregates when braces are
    omitted.
  - Fill nested arrays/records in C declaration order.
  - Preserve explicit braces and designators.
  - Keep later initializer entries overriding earlier designated entries.
- Out:
  - GNU empty initializer `{}`.

## Deliverables
- Local and global initializer tests for nested arrays in structs with omitted
  braces.
- c-testsuite regression for `00205`.

## Acceptance
- `PT cases[] = { 1,2,3,4,5,6,7, ... };` maps the first four values into
  `cases[0].c[0..4]`, then `b`, `e`, `k`.
- `c-testsuite::00205` compiles and executes successfully.

## References
- C99 §6.7.8p20
- `third_party/testsuites/c-testsuite/tests/single-exec/00205.c`
