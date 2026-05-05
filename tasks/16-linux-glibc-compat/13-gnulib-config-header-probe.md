# 16-13: Gnulib Config Header Probe

> ✓ done — 2026-05-06

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-12-dlfcn-and-runtime-linking  
**Milestone:** hosted-linux

## Goal

Parse generated gnulib configuration headers and replacement headers from GNU
coreutils without source edits.

## Scope

- In: host bootstrap/configure logs, generated `lib/config.h`, gnulib include
  paths, and selected replacement headers.
- In: turning parser or type checker failures into precise compiler tasks.
- Out: committing generated coreutils build artifacts.

## Acceptance

- [x] `real_world/projects/09-gnu-coreutils/plan.md` names the generated include
      paths needed for the first rcc compile.
- [x] `rcc -fsyntax-only` can parse generated `config.h` through a wrapper
      translation unit or the remaining failures are task-linked.
- [x] Logs are kept under ignored `logs/` or `build/`, not committed.
- [x] No upstream source files are modified.
