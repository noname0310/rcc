> ✓ done — 2026-05-04 — implemented in commit

# 10-12: Miscellaneous compatibility flags

**Phase:** 10-driver    **Depends on:** —    **Milestone:** M6

## Goal
Implement commonly expected CLI flags for GCC compatibility:
`-std=c99` (standard selection), GNU-mode spelling aliases, and common
`-f` flags that should parse without breaking build-system generated command
lines.

## Scope
- In:
  - `-std=c99`: accept as the default standard. `-std=c11`,
    `-std=gnu99`, etc. are rejected with a clear "unsupported
    standard" error message.
  - `-ansi`: accepted as a compatibility alias that selects strict C89 later,
    but for now emits an "unsupported standard" diagnostic instead of being
    silently ignored.
  - `-f<flag>`: silently accept (with optional warning) common
    flags that do not affect rcc behaviour: `-fPIC`,
    `-fno-strict-aliasing`, `-fwrapv`, `-fstack-protector`,
    `-fno-common`, `-fvisibility=hidden`. Unknown `-f` flags
    produce a warning.
- Out: actual implementation of `-fPIC` codegen, stack protectors.
  Verbose tool tracing is owned by `10-16-tool-discovery-and-verbose-trace.md`.

## Deliverables
- CLI parsing for `-std=`, `-ansi`, and the `-f` family.
- Standard validation.
- Ignored-flag warning infrastructure.
- Tests for each flag category.

## Acceptance
- `rcc -std=c99 hello.c` compiles normally.
- `rcc -std=c11 hello.c` emits "unsupported standard" error.
- `rcc -fPIC hello.c` compiles with a note that the flag is ignored.

## References
- GCC overall options documentation.
