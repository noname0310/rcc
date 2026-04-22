# 11-07: gcc-torture execute ≥ 60 %

**Phase:** 11-conformance    **Depends on:** 11-06    **Milestone:** M6

## Goal
Run the full `gcc.c-torture/execute/` suite (~1200 files). Target
≥ 60 % pass rate by M6. Remaining failures are typically GCC
extensions we deliberately don't support.

## Scope
- In: full adapter; bulk xfail list for extension-reliant tests.
- Out: `compile/` suite variants.

## Deliverables
- Nightly pass-rate report.

## Acceptance
- Pass rate ≥ 60 %, stable for a week of nightly runs.

## References
- Plan §10 M6.
