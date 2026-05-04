# 11-15e: gcc-torture runtime signal triage

**Phase:** 11-conformance    **Depends on:** 11-15    **Milestone:** M6

## Goal
Classify and reduce gcc-torture cases that compile and link but terminate by
signal at runtime.

## Scope
- In: full-report failures with `non-zero exit code: killed by signal`.
- Out: compile-time parser/typeck/codegen failures already covered by 15a-15d.

## Deliverables
- A small matrix of reduced runtime-signal cases with expected behavior under
  clang.
- At least one concrete compiler bug fix or a set of follow-up tasks if the
  cluster splits into independent ABI/evaluation-order/codegen bugs.
- Updated full report with the runtime-signal count reduced or fully explained.

## Acceptance
- Representative cases `20010904-1`, `20010904-2`, `20011113-1`, and
  `20011223-1` are either fixed or classified into narrower follow-up tasks.
- No runtime-signal case is marked xfail without a specific reason tied to a
  non-C99 extension or an already-created compiler-bug task.

## References
- `target/wsl/gcc-torture-full-15-final.json`
