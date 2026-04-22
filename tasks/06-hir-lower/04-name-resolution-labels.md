# 06-04: Label resolution

**Phase:** 06-hir-lower    **Depends on:** 06-01    **Milestone:** M3

## Goal
Labels are per-function; `goto X` must find `X:` somewhere in the same
function body. Forward references are allowed, so we do **two**
passes: collect label → `HirStmtId`, then check every `goto`.

## Scope
- In: `Resolver::labels` cleared per function; unknown label → E0073;
  duplicate label in same function → E0074.
- Out: label-scoped goto restrictions beyond C99 (there are none
  worth enforcing up front).

## Deliverables
- Two-pass label resolver.
- Tests: forward goto, backward goto, undefined label, duplicate.

## Acceptance
- `void f(){ goto x; x:; }` resolves.
- `void f(){ a: b: goto a; }` fine; `void f(){ a:a:; }` error.

## References
- C99 §6.2.3, §6.8.1.
