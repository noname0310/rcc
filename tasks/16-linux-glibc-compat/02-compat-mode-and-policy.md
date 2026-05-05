> ✓ done — 2026-05-06

# 16-02: Compatibility Mode And Policy

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-01-hosted-linux-surface-audit  
**Milestone:** hosted-linux

## Goal

Define the command-line and session-level policy for Linux hosted compilation so
the compiler can be strict C99 by default while still supporting glibc/POSIX/GNU
project builds intentionally.

## Scope

- In: an explicit hosted mode, diagnostics wording, and strictness boundaries.
- In: interaction with `-std=c99`, GNU extension flags, system include paths,
  and `-pthread`.
- Out: implementing each individual header shim.

## Acceptance

- [ ] `rcc --help` documents the hosted Linux mode or equivalent flag surface.
- [ ] `docs/hosted-linux.md` states what `rcc` owns and what host libraries own.
- [ ] Strict mode continues to reject GNU-only syntax unless an existing GNU
      extension flag enables it.
- [ ] A regression test proves strict C99 behavior is unchanged by the new mode.
