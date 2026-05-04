> ✓ done — 2026-05-04

# 11-15k: gcc-torture runtime signal cluster sweep

**Phase:** 11-conformance    **Depends on:** 11-15e    **Milestone:** M6

## Goal
Turn the remaining gcc-torture runtime signal bucket into actionable compiler
bug clusters instead of treating aborts as a single pass-rate number.

## Scope
- In: cases whose report reason is `non-zero exit code: killed by signal`.
- Out: compile/link failures and cases already covered by 15i or 15j.

## Deliverables
- A generated or checked-in markdown table grouping signal cases by root-cause
  class: GNU extension semantics, ABI/calling convention, aggregate layout,
  bit-field signedness/layout, varargs/libcall behavior, vector extension, or
  unknown.
- At least three reduced fixtures with clang/rcc behavior documented.
- New follow-up tasks for any cluster that represents a C99 compiler bug.

## Acceptance
- `target/wsl/gcc-torture-full-15e-after.json` signal cases are grouped with
  concrete reasons instead of a raw count only.
- No case is marked xfail without a specific non-C99-extension reason or an
  already-created compiler-bug task.
- The next conformance task can pick one cluster without re-reading the full
  report from scratch.

## Result
- Added `docs/gcc-torture-signal-clusters.md`.
- Reswept the original 61 signal cases after `11-15i` and `11-15j`:
  9 now pass and 52 still fail by signal.
- Grouped all remaining signal cases by root-cause class and attached a
  follow-up task to every cluster.
- Checked three C99 reductions against host `cc -std=c99` and current rcc:
  block-scope `extern`, VLA parameter side effects, and prefix decrement plus
  compound assignment.
- Added follow-up tasks `11-15l` through `11-15w`.
- Added no xfail, skip, or result masking.

## References
- `target/wsl/gcc-torture-full-15e-after.json`
- `target/wsl/gcc-torture-15k-signal-resweep.json`
- `docs/gcc-torture-signal-clusters.md`
