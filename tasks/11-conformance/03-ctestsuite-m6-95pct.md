# 11-03: c-testsuite @ M6 ≥ 95 %

**Phase:** 11-conformance    **Depends on:** 11-02, 08-13, 09-13    **Milestone:** M6

## Goal
After VLA, compound literal, designated initializer, `inline`,
`restrict`, `_Bool`, and variadic support land, drive c-testsuite to
95 %. The residual 5 % should be genuine corner cases parked under
`xfail.toml` with explicit reasons.

## Scope
- In: bug bash remaining failures; document any standard-relaxation
  decisions (e.g. `long double` precision).
- Out: --.

## Deliverables
- Final xfail trim.
- KPI green.

## Acceptance
- Pass rate ≥ 95 % on 3 consecutive CI runs.

## References
- Plan §10 M6.
