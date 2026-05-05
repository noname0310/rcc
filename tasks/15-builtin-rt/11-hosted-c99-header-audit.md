> ✓ done — 2026-05-05

# 15-11: Hosted C99 header surface audit

**Phase:** 15-builtin-rt    **Depends on:** 15-04    **Milestone:** real-world-02

## Goal
Inventory the compiler-provided hosted C99 declaration shims before adding more
one-off declarations discovered by real-world projects.

## Scope
- In: current `lib/rcc/include/*.h` declarations, missing C99 hosted headers,
  missing declarations inside already-present headers, and prioritized follow-up
  task split.
- Out: implementing libc bodies, copying glibc/musl headers, and declaring
  POSIX/GNU extensions without a concrete extension task.

## Deliverables
- `docs/hosted-c99-header-audit.md`.
- Follow-up tasks for declaration sweeps.

## Acceptance
- The audit distinguishes declaration shims from libc implementation.
- Missing declarations are grouped by header, not discovered one compile error
  at a time.
- Follow-up tasks preserve the rule that project probes must not edit upstream
  `.c` or `.h` files to hide compiler gaps.

## Real-world trigger
`real_world/projects/02-cjson` exposed missing `strtod` and `sscanf`
declarations after `real_world/projects/01-inih` had already exposed an
incomplete `<ctype.h>` shim. That pattern shows the header surface needs an
audit and sweep instead of incremental patching.

