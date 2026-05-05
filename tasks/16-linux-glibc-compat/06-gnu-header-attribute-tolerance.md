# 16-06: GNU Header Attribute Tolerance

> ✓ done — 2026-05-06

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-05-glibc-common-macro-shims  
**Milestone:** hosted-linux

## Goal

Accept the GNU attributes commonly used by glibc and gnulib headers, preserving
semantically important facts and ignoring harmless annotations deliberately.

## Scope

- In: attributes from the audit such as `nothrow`, `leaf`, `nonnull`, `pure`,
  `const`, `malloc`, `format`, `warn_unused_result`, `visibility`, and
  `deprecated`.
- In: diagnostic behavior for malformed or unsupported attributes.
- Out: blanket accepting unknown tokens without parse structure.

## Acceptance

- [x] Attribute parsing has table-driven tests for supported glibc attributes.
- [x] Ignored attributes are recorded or documented as no-op in rcc semantics.
- [x] Unsupported attributes produce a recoverable diagnostic, not parser drift.
- [x] Representative glibc declarations parse in hosted mode.
