# 11-16e: tcc-tests2 flexible array initialization

> ✓ done — 2026-05-04

**Phase:** 11-conformance    **Depends on:** 11-16    **Milestone:** M6

## Goal
Fix the flexible-array initializer/assignment path exposed by tcc-tests2.

## Scope
- In: `tcc-tests2::80_flexarray`.
- Out: GNU zero-length arrays unless the fixture reaches them after the C99
  flexible-array bug is fixed.

## Deliverables
- A reduced semantic test for a struct with a final flexible array member.
- Typeck/HIR/codegen changes needed so legal flexible-array usage is accepted
  and illegal usage still diagnoses cleanly.

## Acceptance
- `80_flexarray` passes or is reclassified only if a specific outside-release
  construct is proven after the C99 bug is gone.
- No generic `expression is not assignable to the required type` is emitted
  for legal flexible-array initialization.

## Result
- Fixed legal C99 flexible-array objects in LLVM codegen: final flexible array
  members are omitted from LLVM record bodies and zero-size global initializers,
  and `p->data[i]` indexes from the record tail pointer.
- Reclassified `tcc-tests2::80_flexarray` as xfail because its static
  initializer initializes the flexible array member itself; GCC rejects that
  under `-std=c99 -pedantic-errors`.
- WSL tcc-tests2 baseline after this task: 88 discovered, 68 pass, 9 xfail,
  7 fail, 4 skip.

## References
- `target/wsl/tcc-tests2-16-final.json`
- `target/wsl/tcc-tests2-16e-final.json`
- C99 §6.7.2.1p16-p18.
