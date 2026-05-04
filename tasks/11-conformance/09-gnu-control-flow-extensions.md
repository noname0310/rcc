# 11-09: GNU control-flow extensions

**Phase:** 11-conformance    **Depends on:** 11-07    **Milestone:** M3+

## Goal
Implement the GNU control-flow extensions that block chibicc `control.c`:
case ranges and labels-as-values / computed goto.

## Scope
- In:
  - Parse, lower, type-check, and CFG-lower `case lo ... hi:`.
  - Parse, lower, type-check, and codegen `&&label` as a label address.
  - Parse, lower, type-check, and codegen `goto *expr`.
  - Keep all three extensions gated by explicit GNU options.
- Out:
  - Non-local label address semantics across functions.
  - Optimizing jump tables.

## Deliverables
- Parser/HIR/CFG/codegen tests for case ranges.
- Parser/HIR/CFG/codegen tests for labels-as-values and computed goto.
- Driver/conformance option wiring for chibicc compatibility mode.

## Acceptance
- `case 0 ... 5:` selects all values in the inclusive range.
- `void *p = &&label; goto *p;` reaches the target label in an E2E test.
- Strict C99 mode still diagnoses or warns according to the configured GNU
  options.

## References
- chibicc `test/control.c`.
- GCC labels-as-values extension.
