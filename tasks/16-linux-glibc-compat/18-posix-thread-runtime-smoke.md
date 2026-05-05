> ✓ done — 2026-05-06

# 16-18: POSIX Thread Runtime Smoke

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-17-gnu-coreutils-single-utility-probe  
**Milestone:** hosted-linux

## Goal

Prove that hosted pthread compilation links to the host implementation and runs
correctly for a minimal program.

## Scope

- In: one checked-in C fixture, one driver test, and one Linux runtime test.
- In: `-pthread` compile macro and link behavior.
- Out: pthread internals.

## Acceptance

- [x] The fixture starts one thread, joins it, and validates a shared result.
- [x] `rcc -pthread` builds and runs the fixture on Linux.
- [x] The same command has a clear unsupported diagnostic on non-Linux targets.
- [x] The test is wired into the hosted Linux gate without source mutation.

## Result

The checked-in fixture is
`crates/rcc_driver/tests/fixtures/pthread_runtime_smoke.c`.  It starts one
thread, joins it, validates the returned pointer, and checks that the worker
updated shared state to `42`.

Coverage:

- `misc_cli_flags::pthread_header_shim_parses_and_typechecks_for_linux_target`
  lowers the fixture with `--target=x86_64-unknown-linux-gnu`,
  `--linux-gnu-hosted`, and `-pthread`.
- `linker_flags::e2e_link_with_pthread_when_enabled` links/runs the same
  fixture on Linux when `RCC_RUN_LINK_E2E=1`.
- `misc_cli_flags::pthread_is_rejected_for_windows_targets` keeps the
  unsupported Windows target diagnostic path active.
