> ✓ done — 2026-05-04

# 11-15: gcc-torture execute ≥ 60 %

**Phase:** 11-conformance    **Depends on:** 11-14    **Milestone:** M6

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
- Pass rate ≥ 60 %, stable across repeated manual runs.

## Result
- `GccTortureAdapter` now has separate smoke and full execute modes.
- Full mode discovers every direct `gcc.c-torture/execute/*.c` case.
- The full manual job fetches gcc-torture behind `--include-gpl`, runs the
  full execute adapter, uploads the JSON report, and fails if pass rate drops
  below 0.600.
- Added `-fgnu-builtin-libcalls` so GCC torture's common builtin libc aliases
  and predefined scalar-limit macros are enabled only in explicit compatibility
  mode, not strict C99 mode.
- Local WSL validation: 1650 discovered, 1104 passed, 546 failed, 0 xfail,
  0 skip, pass rate 0.669.
- No xfail entries were added. Remaining failures are tracked as 15a-15e
  instead of being hidden by the percentage gate.

## References
- Plan §10 M6.
