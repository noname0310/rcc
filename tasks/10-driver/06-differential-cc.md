> ✓ done — 2026-05-04

# 10-06: Differential vs host `cc`

**Phase:** 10-driver    **Depends on:** 10-05    **Milestone:** M3

## Goal
For every E2E fixture, also build with host `cc` (gcc / clang) and
compare both binaries' stdout + exit code. Catches miscompiles that
our own expected-output tests would miss.

## Scope
- In: optional test runner; skip cleanly when `cc` is unavailable;
  use the same command-line as our runner's link step.
- Out: bit-identical object comparison (pointless — ABI allows
  layout differences).

## Deliverables
- Runner test.
- Report file: per fixture, both exit codes.

## Acceptance
- On ≥ 5 fixtures, rcc and cc agree bit-for-bit on stdout.
- Disagreement triggers a clear failure listing both outputs.

## References
- Plan §8.4 "교차검증".
