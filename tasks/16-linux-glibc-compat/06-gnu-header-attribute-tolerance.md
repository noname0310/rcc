# 16-06: GNU Header Attribute Tolerance

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

- [ ] Attribute parsing has table-driven tests for supported glibc attributes.
- [ ] Ignored attributes are recorded or documented as no-op in rcc semantics.
- [ ] Unsupported attributes produce a recoverable diagnostic, not parser drift.
- [ ] Representative glibc declarations parse in hosted mode.
