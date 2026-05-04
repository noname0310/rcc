> ✓ done — 2026-05-04

# 11c: chibicc __va_area__ compatibility

**Phase:** 11-conformance    **Depends on:** 11-11, 11b    **Milestone:** M4+

## Goal
Support the chibicc compatibility builtin `__va_area__` well enough for
`function.c`'s variadic `fmt` helper.

## Scope
- In:
  - Add an explicit GNU/chibicc compatibility option or warning gate for
    `__va_area__`.
  - Resolve `__va_area__` only inside variadic functions.
  - Give it a pointer type compatible with the fixture's cast to
    `(__va_elem *)`.
  - Lower/codegen it to the current function's SysV varargs save-area object
    or an equivalent value accepted by the existing `va_list` layout.
  - Reject or diagnose use outside variadic functions.
- Out:
  - General GCC `__builtin_va_list` compatibility beyond the existing
    `stdarg`/va intrinsic tasks.
  - Windows varargs.

## Deliverables
- Focused HIR/typeck/codegen tests for a reduced `fmt`-style function.
- Driver/conformance flag wiring if a new compatibility option is added.
- E2E or WSL-only LLVM test proving `fmt(buf, "%d", 1)` reaches libc
  `vsprintf` correctly.

## Acceptance
- `function.c` advances past the `__va_area__` undeclared diagnostic.
- `__va_area__` outside a variadic function is rejected.
- Existing C99 `va_start`/`va_arg` tests stay green.

## Notes (agent)
- `function.c` now reaches object/link stage past `__va_area__`; the next
  observed blocker is `inline_fn` link semantics, owned by the following
  conformance task.

## References
- chibicc `test/function.c` line 96.
- SysV x86-64 ABI §3.5.7 variable argument lists.
- `tasks/09-codegen-llvm/19-varargs-va-intrinsics.md`.
- `tasks/15-builtin-rt/03-stdarg-header.md`.
