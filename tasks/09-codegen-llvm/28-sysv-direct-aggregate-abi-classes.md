# 09-28: sysv direct aggregate ABI classes

> ✓ done — 2026-05-04

**Phase:** 09-codegen-llvm    **Depends on:** 09-08, 09-13, 09-27    **Milestone:** M6+

## Goal
Fix the `c-testsuite::00204` compile failure where SysV aggregate
return/argument classification reaches a direct ABI class that rcc cannot
lower into LLVM IR. This task removes the internal codegen error; runtime
aggregate `va_arg` fallout is split into `09-29`.

## Trigger
- `c-testsuite::00204` currently fails during LLVM codegen with:
  `failed to lower HIR type TyId(78) to LLVM: unsupported direct ABI class`.

## Scope
- In:
  - Minimize which aggregate shape produces the unsupported direct ABI class.
  - Extend direct ABI unit lowering for the SysV classes that are valid for
    small C99 aggregate returns/arguments on the current Linux target.
  - Add LLVM regression coverage and a focused `00204` compile check.
- Out:
  - Full cross-target ABI conformance.
  - Windows ABI support.
  - Non-C99 extension cases.

## Deliverables
- Codegen tests for the previously unsupported ABI class.
- `c-testsuite::00204` no longer fails with an internal codegen error.
- `09-29` owns the newly exposed aggregate `va_arg` runtime crash.
- Updated conformance dashboard if the pass/fail count changes.

## Acceptance
- Focused minimizer reproduces the old failure and passes after the fix.
- `c-testsuite::00204` compiles far enough to expose the next runtime bug
  instead of failing with `unsupported direct ABI class`.
- Existing SysV ABI golden tests remain green.
- Full c-testsuite pass/fail counts do not regress.

## References
- `third_party/testsuites/c-testsuite/tests/single-exec/00204.c`
- System V AMD64 ABI aggregate classification
