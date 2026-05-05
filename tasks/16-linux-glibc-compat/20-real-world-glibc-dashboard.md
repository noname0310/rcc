> ✓ done — 2026-05-06

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

- [x] The dashboard shows pass/fail/blocker per project stage.
- [x] GNU coreutils has separate bootstrap, syntax, object, link, and runtime
      cells.
- [x] Every red cell names a concrete compiler-owned issue or a host tool
      prerequisite.
- [x] `tasks/index.md` can flip phase 16 only after this dashboard is current.

## Result

The dashboard is `real_world/hosted-linux-dashboard.md`.

It records stage-level status for:

- MuJS: header/config, Syntax/HIR, Object, Link, and Runtime are PASS.
- GNU coreutils `src/true`: bootstrap/configure and generated headers are PASS;
  Syntax/HIR is now BLOCKED by `16-22`; object/link/runtime remain blocked
  until `16-22` through `16-24` land.

`tasks/index.md` is intentionally not flipped for phase 16 because this
dashboard is current but compiler-owned follow-ups remain pending.
