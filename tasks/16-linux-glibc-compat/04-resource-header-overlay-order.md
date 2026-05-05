> ✓ done — 2026-05-06

# 16-04: Resource Header Overlay Order

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-03-feature-test-macro-model  
**Milestone:** hosted-linux

## Goal

Make rcc resource headers and shims participate in include search without
silently replacing ordinary host headers.

## Scope

- In: include search order for rcc resources, selected overlay headers, project
  `-I`, system `-isystem`, and host defaults.
- In: tests for quoted and angle includes.
- Out: copying large glibc or Linux kernel headers.

## Acceptance

- [ ] Include tracing shows the exact file chosen for every overlay candidate.
- [ ] Small rcc shim headers can shadow selected problematic host headers.
- [ ] Normal host headers remain discoverable after the shim layer.
- [ ] A regression test proves a project `-I` include is not stolen by the
      resource directory.
