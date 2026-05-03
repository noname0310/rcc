# 10-00.1: Pre-driver task-state hygiene

**Phase:** 10-driver    **Depends on:** 09-26    **Milestone:** M3    **Size:** Small

## Goal

Make the completed 01-09 task tree mechanically trustworthy before new driver
agents start consuming it. Phase indexes already mark those tasks complete, but
some completed task files are missing the required `> ✓ done — YYYY-MM-DD`
banner.

## Scope

- In:
  - Audit `tasks/index.md` and `tasks/01-*` through `tasks/09-*`.
  - Add missing `> ✓ done — YYYY-MM-DD` banners to task files whose phase
    index entry is `[x]`.
  - Keep `tasks/index.md` and every completed phase index synchronized.
  - Add a small scripted check if useful, but do not create a broad task
    management system.
- Out:
  - Changing implementation code.
  - Reopening completed task acceptance criteria.
  - Editing `.cursor/plans/*`.

## Deliverables

- Missing done banners backfilled for every `[x]` task in completed phases.
- Optional `xtask` or script check that can fail on future state drift.
- A short note in the task file explaining this was metadata hygiene only.

## Acceptance

- Every `[x]` task listed in phase indexes 01 through 09 has a done banner.
- No `[~]` entries remain in completed phase indexes.
- `tasks/index.md` still marks 01 through 09 as `[x]`, and 10-driver remains
  the first pending implementation phase.
- Worktree diff contains only task metadata / optional checker code.

## References

- `agent.md` state protocol.
- Review finding before driver phase: completed indexes were correct, but a
  subset of completed task files lacked the done banner.
