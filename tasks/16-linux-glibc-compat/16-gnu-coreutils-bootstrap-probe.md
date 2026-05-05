# 16-16: GNU Coreutils Bootstrap Probe

> ✓ done — 2026-05-06

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-15-mujs-hosted-smoke  
**Milestone:** hosted-linux

## Goal

Run and document GNU coreutils bootstrap/configure with the host toolchain so the
generated gnulib configuration surface becomes reproducible input for rcc.

## Scope

- In: `real_world/projects/09-gnu-coreutils/upstream`, a local build directory,
  generated include path inventory, and host compiler baseline.
- In: detecting missing local tools and recording them clearly.
- Out: using rcc as the configure compiler in this task.

## Acceptance

- [x] `real_world/projects/09-gnu-coreutils/plan.md` is updated with exact host
      bootstrap/configure commands.
- [x] The generated `config.h` location and include order are recorded.
- [x] A small host-built utility command is recorded as the runtime oracle.
- [x] Generated files stay ignored under `build/`, `logs/`, or `scratch/`.
