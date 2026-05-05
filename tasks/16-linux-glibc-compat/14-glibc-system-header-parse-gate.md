# 16-14: Glibc System Header Parse Gate

> ✓ done — 2026-05-06

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-13-gnulib-config-header-probe  
**Milestone:** hosted-linux

## Goal

Add a repeatable gate that parses and type-checks representative hosted Linux
headers before real-world project probes depend on them.

## Scope

- In: compile-only fixture files including headers such as `<stdio.h>`,
  `<stdlib.h>`, `<unistd.h>`, `<sys/types.h>`, `<sys/stat.h>`, `<fcntl.h>`,
  `<dirent.h>`, `<pthread.h>`, and `<dlfcn.h>`.
- In: WSL/Linux-only gating where host headers are required.
- Out: requiring these host headers on Windows.

## Acceptance

- [x] The gate runs in CI on Linux or is clearly marked as Linux-only.
- [x] Every fixture has a reason tied to a real project.
- [x] Failures are not ignored with broad `xfail`; each has a task link.
- [x] The gate is referenced from `docs/hosted-linux.md`.
