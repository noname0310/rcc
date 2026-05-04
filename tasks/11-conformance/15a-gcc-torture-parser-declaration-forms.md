# 11-15a: gcc-torture parser declaration forms

**Phase:** 11-conformance    **Depends on:** 11-15    **Milestone:** M6

## Goal
Turn the largest remaining gcc-torture parser failure cluster into concrete
parser fixes instead of treating the 60% gate as sufficient.

## Scope
- In: failures reported as `expected ';' after declaration`, `expected ')' to
  close parameter list`, and closely related declaration/declarator recovery
  cases in `gcc.c-torture/execute`.
- Out: C11-only constructs that are not required for the C99 compiler; if a
  failing case is C11-only, document it and keep it out of strict gates.

## Deliverables
- Minimal parser unit or UI tests extracted from representative failing cases.
- Parser fixes or explicit compatibility flags for GNU-only declaration syntax.
- Updated gcc-torture full report showing the cluster reduced.

## Acceptance
- Representative cases such as `20000822-1`, `20001024-1`, `20001121-1`,
  and `20020108-1` no longer fail with parser shape errors unless classified
  as out-of-scope non-C99 syntax.
- No xfail entries are added for ordinary C99 parser bugs.

## References
- `target/wsl/gcc-torture-full-15-final.json`
