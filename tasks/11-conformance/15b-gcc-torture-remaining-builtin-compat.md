# 11-15b: gcc-torture remaining builtin compatibility

> ✓ done — 2026-05-04

**Phase:** 11-conformance    **Depends on:** 11-15f    **Milestone:** M6

## Goal
Handle the remaining GCC builtin compatibility failures that are not part of
strict C99 but are common in gcc-torture execute fixtures.

## Scope
- In: `__builtin_printf`, `__builtin_prefetch`, `__builtin_mul_overflow`, and
  any adjacent builtin libc aliases surfaced by the full report.
- Out: arbitrary GCC builtin surface area not exercised by current fixtures.

## Deliverables
- Compatibility-mode lowering or macro/prototype wiring behind explicit flags.
- Focused tests proving strict C99 still rejects the names while compatibility
  mode accepts them.
- gcc-torture rerun showing the targeted builtin failures reduced.

## Acceptance
- `builtin-prefetch-1` through `builtin-prefetch-6` no longer fail as
  undeclared identifiers in compatibility mode.
- Representative `__builtin_printf` fixtures no longer fail as undeclared
  identifiers in compatibility mode.
- `__builtin_mul_overflow` is either implemented with correct runtime
  semantics or split into a more specific codegen/typeck task.

## Result
- Added explicit `-fgnu-builtin-libcalls` aliases for common libc-style
  builtins: `__builtin_printf`, `__builtin_sprintf`,
  `__builtin_snprintf`, `__builtin_malloc`, `__builtin_alloca`,
  `__builtin_strcpy`, `__builtin_strncpy`, and `__builtin_strchr`.
- Added a variadic `__builtin_prefetch(addr, ...)` predefined macro that
  preserves the side effects of the address expression by expanding to
  `((void)(addr))`.
- Injected matching external prototypes for the libc targets behind the same
  compatibility flag; strict C99 mode still leaves these names undeclared.
- Fixed SysV ABI classification for `Ty::BuiltinVaList` parameters so injected
  `vprintf`/`vfprintf` declarations do not panic codegen.
- Split arithmetic overflow builtins into
  `15g-gcc-torture-overflow-builtins.md` because they require real store +
  boolean overflow semantics rather than aliases.
- Split the new `20020406-1` post-alias blocker into
  `15h-gcc-torture-record-typedef-field-lowering.md`.
- WSL probes: `builtin-prefetch-1` through `builtin-prefetch-6` all pass.
  `20020406-1` no longer fails as undeclared `__builtin_printf`; it now
  exposes the 15h record typedef lowering bug.
- WSL full `gcc-torture` execute run: 1650 cases, 1165 pass, 485 fail,
  0 xfail, 0 skip; pass_rate=0.706. Builtin-ish undeclared failures dropped
  from 92 to 48.

## References
- `target/wsl/gcc-torture-full-15-final.json`
- `target/wsl/gcc-torture-15b-builtin-probes.json`
- `target/wsl/gcc-torture-full-15b-final.json`
