# 09-29: aggregate va_arg lowering

**Phase:** 09-codegen-llvm    **Depends on:** 09-19, 09-28    **Milestone:** M6+

## Goal
Implement correct SysV lowering for `va_arg(ap, aggregate_type)` so C99
programs that read struct values from variadic argument lists do not crash or
read from null.

## Trigger
- After `09-28`, `c-testsuite::00204` compiles but segfaults inside
  `myprintf`.
- The emitted IR for `va_arg(ap, struct hfa31)` stores `ptr null` and then
  memcpy-loads from that null pointer.

## Scope
- In:
  - Replace or wrap LLVM `va_arg` emission for aggregate types where inkwell
    currently materializes an unusable null pointer value.
  - Use the project SysV ABI classification to advance the va_list correctly
    for memory-passed aggregate varargs.
  - Add focused runtime coverage for at least one aggregate vararg shape.
- Out:
  - Non-SysV varargs.
  - Windows varargs.
  - Non-C99 extension varargs.

## Deliverables
- Codegen helper for aggregate `va_arg` materialization.
- Regression test that rejects `ptr null` aggregate `va_arg` IR.
- `c-testsuite::00204` runs and matches expected stdout, unless a later
  independent bug is exposed and explicitly split.

## Acceptance
- `va_arg(ap, struct { long double x; })` does not lower through `ptr null`.
- Focused aggregate-vararg executable exits successfully.
- `c-testsuite::00204` no longer segfaults in `myprintf`.
- Full c-testsuite run has no new failures.

## References
- `third_party/testsuites/c-testsuite/tests/single-exec/00204.c`
- System V AMD64 ABI variable argument lists
