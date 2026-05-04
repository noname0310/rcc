# 11-15u: gcc-torture GNU inline asm and instrumentation

**Phase:** 11-conformance    **Depends on:** 11-15k    **Milestone:** M6

## Goal
Separate supported GNU inline asm semantics from attributes that are currently
only parsed.

## Scope
- In: `20030222-1`, `990130-1`, `pr49279`, `pr85156`, `eeprof-1`.
- Out: target-specific asm templates beyond x86-64 smoke support.

## Deliverables
- Reductions for output operands, read/write operands, memory clobbers, and
  `no_instrument_function`.
- A decision for `-finstrument-functions`: implement, ignore with warning, or
  exclude from current GNU compatibility mode.

## Acceptance
- Inline-asm cases either pass or have narrower tasks per operand/clobber kind.
- Instrumentation cases are not counted as ordinary C99 compiler bugs.

## References
- `docs/gcc-torture-signal-clusters.md`
