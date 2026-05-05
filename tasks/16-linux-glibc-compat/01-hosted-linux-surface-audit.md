> ✓ done — 2026-05-06

# 16-01: Hosted Linux Surface Audit

**Phase:** 16-linux-glibc-compat  
**Depends on:** 15-builtin-rt  
**Milestone:** hosted-linux

## Goal

Create one reviewed inventory of the Linux hosted surfaces that real projects
already expose, with GNU coreutils as the primary glibc-heavy anchor.

## Scope

- In: MuJS, GNU coreutils, SQLite, zlib, Lua, and any existing real-world logs.
- In: classify failures as language, preprocessor, system-header, driver,
  linker, or runtime-library issues.
- Out: implementing fixes.

## Acceptance

- [ ] `real_world/hosted-linux-audit.md` lists every known hosted blocker with a
      repro command or log path.
- [ ] GNU coreutils has a dedicated section that names the first target utility.
- [ ] Every compiler-owned blocker links to an existing or newly created task.
- [ ] Runtime-owned symbols are explicitly marked as host libc/libm/libpthread
      responsibilities, not rcc implementations.
