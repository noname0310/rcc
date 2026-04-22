# 09-13: Variadic function support

**Phase:** 09-codegen-llvm    **Depends on:** 09-04, 09-05    **Milestone:** M6

## Goal
Support variadic functions (callable + implementable):
- Function type carries `isVarArg = true` in LLVM.
- `va_start` / `va_end` / `va_arg` / `va_copy` map to LLVM intrinsics.
- `stdarg.h`'s `va_list` is a target-specific opaque struct; we just
  let LLVM's intrinsics handle the SysV representation.

## Scope
- In: intrinsic emission + declare required prototypes on first use.
- Out: callee-side handling of variadic forwarding (rare).

## Deliverables
- `emit_va_start` etc.
- Fixture: `printf`-style trampoline.

## Acceptance
- Running a fixture that sums `va_arg(ap, int)` produces the correct
  sum (differential against host cc).

## References
- LLVM LangRef `va_arg`; SysV ABI §3.5.7.
