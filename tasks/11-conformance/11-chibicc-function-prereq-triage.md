# 11-11: chibicc function.c prerequisite triage

> ✓ done — 2026-05-04

**Phase:** 11-conformance    **Depends on:** 11-08    **Milestone:** M4+

## Goal
Turn chibicc `function.c` from an opaque large-fixture failure into a tracked
checklist of concrete frontend, ABI, runtime, and extension blockers.

## Scope
- In:
  - Run `function.c` in the stage-isolated mode and classify each failure by
    owner crate.
  - Separate C99-required compiler bugs from GNU extension blockers and
    runtime/header gaps.
  - Add or update follow-up tasks in the owning phase when the issue is not
    appropriate for phase 11.
- Out:
  - Making `function.c` green.

## Deliverables
- A failure matrix in this task file or a linked conformance note.
- New focused tasks for any blocker not already tracked.
- No xfail entries that hide the selected TU.

## Acceptance
- Every current `function.c` failure maps to an owner and next task.
- C99-required compiler bugs are not treated as pass-rate noise.
- The next actionable implementation task is unambiguous.

## References
- chibicc `test/function.c`.
- `tasks/15-builtin-rt/`
- ABI-related tasks in `09-codegen-llvm/` and `10-driver/`.

## Triage Evidence

Command:

```sh
rcc --emit=mir \
  -fgnu-binary-literals \
  -fgnu-statement-expressions \
  -fgnu-omitted-conditional-operand \
  -fgnu-conditional-void-operand \
  -fgnu-case-ranges \
  -fgnu-labels-as-values \
  -fgnu-lvalue-comma \
  -I third_party/testsuites/chibicc/test \
  third_party/testsuites/chibicc/test/function.c
```

Observed compiler diagnostics:

| Lines | Symptom | Classification | Owner | Follow-up |
|---|---|---|---|---|
| 96 | `__va_area__` undeclared in variadic `fmt` | chibicc/GNU compatibility builtin, not C99 `stdarg.h` | HIR lower + typeck + LLVM codegen + session flag | `11c-function-va-area-compat.md` |
| 118, 289, 290 | `__func__` undeclared | C99-required predefined function-scope identifier | parser/HIR lower/typeck/codegen | `11b-function-name-predefined-identifiers.md` |
| 122, 292 | `__FUNCTION__` undeclared | GNU alias for function name string | parser/HIR lower/typeck/codegen, GNU-gated warning | `11b-function-name-predefined-identifiers.md` |
| link-time future blocker | `true_fn`, `false_fn`, `add_all`, `add_float`, `struct_test4`, ... are declared in `function.c` but defined in chibicc `test/common` | conformance harness gap: stage-isolated mode currently supplies only `assert` | `rcc_conformance::ChibiccAdapter` | `11a-function-stage-common-support.md` |
| runtime future blocker | float/double calls, many integer/float args, small/large struct args and returns, long double casts | SysV ABI conformance surface; many subtasks already exist but need one `function.c`-shaped smoke gate | LLVM codegen | `11d-function-abi-runtime-smoke.md` |

Current `--case chibicc::function` is blocked before link/run by the first
three frontend/builtin diagnostics. No xfail was added.

## Follow-Up Order

1. `11a-function-stage-common-support.md`: make the harness capable of linking
   host-compiled chibicc `test/common` for `function.c` without routing it
   through `rcc`.
2. `11b-function-name-predefined-identifiers.md`: implement C99 `__func__` and
   GNU `__FUNCTION__`.
3. `11c-function-va-area-compat.md`: implement the chibicc compatibility
   `__va_area__` builtin needed by `fmt`.
4. `11d-function-abi-runtime-smoke.md`: run reduced ABI slices before attempting
   the full `function.c` gate.
5. `12-chibicc-function-green.md`: only then make the full TU pass.
