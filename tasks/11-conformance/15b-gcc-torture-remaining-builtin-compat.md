# 11-15b: gcc-torture remaining builtin compatibility

**Phase:** 11-conformance    **Depends on:** 11-15    **Milestone:** M6

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

## References
- `target/wsl/gcc-torture-full-15-final.json`
