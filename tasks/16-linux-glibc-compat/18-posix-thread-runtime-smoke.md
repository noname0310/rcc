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

- [ ] The fixture starts one thread, joins it, and validates a shared result.
- [ ] `rcc -pthread` builds and runs the fixture on Linux.
- [ ] The same command has a clear unsupported diagnostic on non-Linux targets.
- [ ] The test is wired into the hosted Linux gate without source mutation.
