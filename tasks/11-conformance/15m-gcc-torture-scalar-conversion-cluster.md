# 11-15m: gcc-torture scalar conversion cluster

**Phase:** 11-conformance    **Depends on:** 11-15k    **Milestone:** M6

## Goal
Separate true C99 scalar conversion bugs from GCC/undefined edge cases in the
remaining scalar runtime signal cases.

## Scope
- In: `20030916-1`, `990222-1`, `20031003-1`, `20060110-1`, `20060110-2`.
- Out: vector conversions and bit-field precision arithmetic.

## Deliverables
- Reduced host-cc/rcc comparisons for unsigned-char wrapping and compound
  assignment sequencing.
- A decision table for signed-shift and out-of-range float-to-int cases:
  C99 bug, GNU compatibility task, or non-portable gcc-torture case.
- Fixes for cases proven to be C99 compiler bugs.

## Acceptance
- `990222-1` and `20030916-1` are either passing or have narrower compiler-bug
  tasks.
- Undefined/GNU-only cases are documented without changing pass-rate accounting.

## References
- `docs/gcc-torture-signal-clusters.md`
