> ✓ done — 2026-05-04 — implemented in commit

# 10-09: Warning control flags

**Phase:** 10-driver    **Depends on:** 10-03    **Milestone:** M6

## Goal
Implement GCC-compatible warning control flags: `-Wall`, `-Wextra`,
`-Werror`, `-Wno-<name>`, `-Wpedantic`. Add a `WarningConfig` to
`Options` that the diagnostic handler uses to filter or promote
diagnostics.

## Scope
- In: CLI parsing for `-W` family flags. `WarningConfig` struct:
  enabled warning groups (all, extra, pedantic), individual
  warning overrides (`-Wno-unused-variable`), error promotion
  (`-Werror`, `-Werror=<name>`). Wire into `Handler` so that
  diagnostics are filtered by their warning code before emission.
  `-w` to suppress all warnings.
- Out: defining which warnings belong to which group (task 13-07).

## Deliverables
- `WarningConfig` struct in options.
- CLI parsing for all `-W` variants.
- `Handler` integration: filter/promote diagnostics.
- Tests: `-Werror` promotes warning to error, `-w` suppresses.

## Acceptance
- `-Werror` causes the compiler to exit with error status on any
  warning.
- `-Wno-unused-variable` suppresses that specific warning.
- `-Wall` enables the standard warning set.
- `-w` suppresses all warnings.

## References
- GCC warning options documentation.
