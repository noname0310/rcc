# 10-18: Dependency file emission

> ✓ done — 2026-05-04 — implemented in commit

**Phase:** 10-driver    **Depends on:** 04-03, 10-07, 10-14    **Milestone:** M5    **Size:** Medium

## Goal

Emit make-compatible dependency files so `rcc` can participate in ordinary C
build systems.

## Scope

- In:
  - CLI flags: `-M`, `-MM`, `-MD`, `-MMD`, `-MF <file>`, `-MT <target>`,
    `-MQ <target>`.
  - Preprocessor include tracking from the existing include search machinery.
  - Escaping spaces, `#`, `$`, and backslashes in makefile targets/prereqs.
  - Stop-flag interaction with `-E`, `-c`, and default output naming.
- Out:
  - System-header classification beyond what include search already records.
  - Modules / header units.

## Deliverables

- Include-dependency collection API in the driver/preprocessor boundary.
- Makefile dependency renderer.
- Tests using nested local includes and missing include diagnostics.

## Acceptance

- `rcc -MMD -MF hello.d -c hello.c` writes a dependency file and an object.
- `rcc -M hello.c` writes dependencies to stdout and does not compile.
- Escaped targets round-trip in make-compatible syntax.

## References

- GCC dependency generation flags
- Clang dependency generation flags
