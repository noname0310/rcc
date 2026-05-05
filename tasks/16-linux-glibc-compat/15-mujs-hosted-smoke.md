# 16-15: MuJS Hosted Smoke

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-14-glibc-system-header-parse-gate  
**Milestone:** hosted-linux

## Goal

Record and automate the already proven MuJS compile-and-run smoke as the small
hosted project regression before moving to GNU coreutils.

## Scope

- In: host and rcc commands for `main.c` + `one.c`, runtime script output, and
  reproducible wrapper script.
- In: no source edits.
- Out: expanding MuJS beyond the smoke unless it exposes a compiler bug.

## Acceptance

- [ ] `real_world/projects/07-mujs/plan.md` records upstream commit, commands,
      and observed output.
- [ ] A wrapper script rebuilds and runs the `print(1+2)` smoke.
- [ ] `real_world/results.md` or equivalent dashboard records success.
- [ ] The smoke is not allowed to hide compiler diagnostics with source edits.
