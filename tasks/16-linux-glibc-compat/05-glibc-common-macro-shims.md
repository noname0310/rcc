# 16-05: Glibc Common Macro Shims

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-04-resource-header-overlay-order  
**Milestone:** hosted-linux

## Goal

Provide minimal declarations or macro compatibility for common glibc internal
annotation macros that block parsing but do not change C semantics for rcc.

## Scope

- In: `__THROW`, `__THROWNL`, `__nonnull`, `__wur`, `__BEGIN_DECLS`,
  `__END_DECLS`, `__attribute_malloc__`, and similar annotation wrappers found
  by the audit.
- In: unit tests that include these macros in function declarations.
- Out: pretending to implement glibc internals.

## Acceptance

- [ ] The shim layer defines the audited macros only when needed.
- [ ] Tests cover function declarations before and after macro expansion.
- [ ] GNU coreutils `system.h` gets past annotation macros without source edits.
- [ ] All added shims are documented as parse/type compatibility only.
