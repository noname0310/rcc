# 16-20: Real-World Glibc Dashboard

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-19-header-shim-audit-docs  
**Milestone:** hosted-linux

## Goal

Make the hosted Linux real-world status visible without reducing failures to a
misleading percentage.

## Scope

- In: MuJS and GNU coreutils rows, commands, current blocker, and next compiler
  task.
- In: CI or local-script references where available.
- Out: marking a project green because a subset happens to compile.

## Acceptance

- [ ] The dashboard shows pass/fail/blocker per project stage.
- [ ] GNU coreutils has separate bootstrap, syntax, object, link, and runtime
      cells.
- [ ] Every red cell names a concrete compiler-owned issue or a host tool
      prerequisite.
- [ ] `tasks/index.md` can flip phase 16 only after this dashboard is current.
