# Agent entry point — `rcc` C99 compiler

You are an engineering agent on **rcc**, a Rust C99 compiler. The
repository is driven by a strict task tree under [`tasks/`](tasks/).
Your job each session is: pick **one** task, execute it, mark it done,
stop. Coordination across agents happens through checkbox state files
— **not** side-channel chat.

Read this file first. Everything else is scoped by the reading rules
below.

## Minimal context (allowlist)

Reading budget is real; do not explore the repo. For the current
session the only files you may open are:

1. `agent.md` (this file).
2. `tasks/index.md` — shows which phase is active.
3. `tasks/<active-phase>/index.md` — shows which task is next.
4. `tasks/<active-phase>/<NN-task>.md` — the actual task spec.
5. Any file the task's *References* section explicitly names.
6. Source files you are about to edit **and their direct imports**.
7. If the task involves running tests/commands, the relevant
   `Cargo.toml` and existing test files it extends.

Explicit denylist:

- `.cursor/plans/*.plan.md` — canonical design, already distilled
  into tasks. Agents must not read it.
- Other phases' `README.md` / `index.md` / task files (except step 2
  to identify the active phase).
- Other tasks within the active phase that you are **not** claiming.
- `target/` and `Cargo.lock`.

If in doubt, *less reading is better*.

## The one cycle

```
1. Open agent.md (this file).
2. Open tasks/index.md.
      ├─ Find the first line matching `- [ ] <NN-phase>`.
      └─ That is the active phase.
3. Open tasks/<active-phase>/index.md.
      ├─ Find the first `- [ ] <NN-task>`.
      └─ That is your task.
4. CLAIM IT ATOMICALLY:
      Edit the checkbox `- [ ]` → `- [~]` in step 3's index.md.
      This is your first write of the session. Any other agent seeing
      `[~]` must skip past it and pick a later task.
5. Open tasks/<active-phase>/<NN-task>.md and read it end-to-end.
6. Verify upstream deps:
      For every id in its `**Depends on:**` line, confirm the
      matching checkbox in that phase's index.md is `[x]`.
      If any dep is NOT `[x]`, revert your claim (`[~]` → `[ ]`),
      leave a one-line `## Notes (agent)` at the bottom of the task
      file naming the blocking dep, and STOP.
7. Write failing tests that encode the task's *Acceptance* bullets
   BEFORE writing production code.
8. Implement the task.
9. Run every gate:
      • `cargo fmt --all --check`
      • `cargo clippy --workspace --all-targets -- -D warnings`
      • `cargo test --workspace`
      • Anything named in the task's *Acceptance* section
        (e.g. `cargo test -p rcc_lexer --test corpus`).
10. On fully green:
      a. Flip the checkbox in the phase index.md: `[~]` → `[x]`.
      b. Prepend this banner at the very top of the task .md
         (above its `# ` heading), on its own line:
            > ✓ done — YYYY-MM-DD — <commit-sha-or-pr>
      c. If that was the last `[ ]` in the phase, flip the phase
         line in tasks/index.md from `[ ]` to `[x]`.
11. STOP. One task per session. Do not claim a second.
```

## State markers (canonical)

In any `index.md`:

| Marker | Meaning |
|--------|---------|
| `- [ ] 03-x-name` | pending; first-in-order is "next". |
| `- [~] 03-x-name` | in-progress; someone is working on it. Skip. |
| `- [x] 03-x-name` | done. Corresponding task file has `✓ done` banner. |

At the task-file level, `> ✓ done — …` as the first line is the
machine-readable completion signal.

## Guardrails

- **Never touch `.cursor/plans/*`.** Design is frozen there.
- **Never edit another task's checkbox.** You may only move your own
  claim forward (`[ ]` → `[~]` → `[x]`) or revert it (`[~]` → `[ ]`).
- **Never skip a pending task** to grab a later one. Dependencies are
  encoded by order.
- **Never merge your own PR.** CI green is required, human review is
  still the merge gate.
- **Do not add new xfail entries** unless the task explicitly permits
  it; existing xfails only shrink.
- **Do not read more files than the allowlist.** If you need more
  context, that means the task description is incomplete — record
  the gap under `## Notes (agent)` and revert your claim.

## Stuck / blocked / ambiguous

- **Compile error you cannot fix** → revert claim, add 2-3 line
  `## Notes (agent)` entry, stop.
- **Spec ambiguity** → revert claim, leave a note with the exact
  C99 §-reference that is unclear, stop.
- **Test suite fetch fails** → run `cargo xtask fetch-testsuites`
  once. If still broken, revert and report.
- **An upstream task you need is only partially done** → revert and
  wait. Do not ghost-fix another task.

## Where to look for deeper context (only if a task points there)

- Working agreement / PR rules: [`tasks/00-overview/03-working-agreement.md`](tasks/00-overview/03-working-agreement.md).
- KPI numbers per milestone: [`tasks/00-overview/02-kpi-dashboard.md`](tasks/00-overview/02-kpi-dashboard.md).
- Glossary of terms: [`tasks/00-overview/04-glossary.md`](tasks/00-overview/04-glossary.md).
- Phase dependency DAG: [`tasks/00-overview/01-phase-ordering.md`](tasks/00-overview/01-phase-ordering.md).

These are NOT part of the default reading list. Open them only when
a task's *References* explicitly links to one.

## TL;DR

Open `tasks/index.md`. Find first `[ ]` phase. Open its `index.md`.
Find first `[ ]` task. Flip to `[~]`. Read task file. Build tests.
Implement. Green gates. Flip to `[x]`. Banner the task file. Stop.
