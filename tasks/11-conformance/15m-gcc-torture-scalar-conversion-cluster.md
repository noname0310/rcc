# 11-15m: gcc-torture scalar conversion cluster

> ✓ done — 2026-05-04

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

## Result

| Case | Classification | Follow-up |
|------|----------------|-----------|
| `990222-1` | C99 compiler-bug candidate. The reduced return-code form passes, but the original abort-shaped torture case still reports a runtime signal under the adapter. Needs focused control-flow/call lowering investigation around `(*--ptr += 1) > '9'` in a short-circuit condition. | `15m1-gcc-torture-990222-assignment-result-control.md` |
| `20030916-1` | C99 compiler-bug candidate. The scoped case exercises `unsigned char` wrapping through compound assignment and array indexing. Needs a reduced runtime fixture independent of the larger 256-element loop. | `15m2-gcc-torture-20030916-uchar-index-wrap.md` |
| `20031003-1` | Non-portable gcc-torture edge. `(int)2147483648.0f` is outside the representable `int` range, so C99 §6.3.1.4 leaves behavior undefined. | none |
| `20060110-1` | Non-portable signed-shift edge. The expression shifts a signed `long long` into a value not representable in the result type; this is not a mandatory C99 behavior. | none |
| `20060110-2` | Same signed-shift family as `20060110-1`, with addition before the shift. | none |

No xfail, skip, or pass-rate masking was added.

## References
- `docs/gcc-torture-signal-clusters.md`
