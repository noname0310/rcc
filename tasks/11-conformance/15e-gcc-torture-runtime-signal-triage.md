# 11-15e: gcc-torture runtime signal triage

> ✓ done — 2026-05-04

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

## Result

| Case | Current status | Classification |
| --- | --- | --- |
| `20010904-1` | fail, SIGABRT | GNU `aligned(32)` layout semantics; follow-up `15i` |
| `20010904-2` | fail, SIGABRT | GNU `aligned(32)` layout semantics; follow-up `15i` |
| `20011113-1` | fail, SIGABRT | aggregate copy / by-value ABI runtime bug; follow-up `15j` |
| `20011223-1` | pass | fixed by applying integer promotions to `switch` conditions |

- Fixed a C99 compiler bug: `switch ((signed char)i)` now promotes the
  controlling expression to `int` before CFG/codegen, so `case 255` no longer
  matches `-1`.
- Added a typeck regression test proving switch conditions receive an
  `IntegerPromotion` wrapper while case values remain in the promoted integer
  domain.
- Added follow-up tasks `15i`, `15j`, and `15k` for the remaining signal
  clusters instead of marking them xfail.
- Full WSL gcc-torture after this task:
  `1215 pass / 435 fail / 0 xfail / 0 skip`; runtime-signal bucket is still
  present and is explicitly carried by `15k`.

## References
- `target/wsl/gcc-torture-full-15-final.json`
- `target/wsl/gcc-torture-15e-probe-before.json`
- `target/wsl/gcc-torture-15e-probe-after.json`
- `target/wsl/gcc-torture-full-15e-after.json`
