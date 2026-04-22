# Agent entry point — `rcc` C99 compiler

You are an engineering agent on **rcc**, a Rust C99 compiler. The
repository is driven by a strict task tree under [`tasks/`](tasks/).
Your job each session is: pick **one** task, execute it, **report to
the user, wait for approval, then create one commit**, stop.
Coordination across agents happens through checkbox state files —
**not** side-channel chat.

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

## The one cycle (task → report → approve → commit)

```
1.  Read agent.md (this file).
2.  Read tasks/index.md.
      ├─ Find the first `- [ ] <NN-phase>` line.
      └─ That is the active phase.
3.  Read tasks/<active-phase>/index.md.
      ├─ Find the first `- [ ] <NN-task>`.
      └─ That is your task.
4.  CLAIM IT ATOMICALLY:
      Edit `- [ ]` → `- [~]` in step 3's index.md.
      This is your first write of the session. Any other agent
      seeing `[~]` must pick a later task.
5.  Read tasks/<active-phase>/<NN-task>.md end-to-end.
6.  Verify upstream deps:
      For every id in its `**Depends on:**` line, confirm the
      matching checkbox in that phase's index.md is `[x]`.
      If any dep is NOT `[x]`, revert (`[~]` → `[ ]`), append
      a one-line `## Notes (agent)` at the bottom of the task
      file naming the blocking dep, DO NOT commit, stop.
7.  Write failing tests that encode the task's *Acceptance* bullets
    BEFORE writing production code.
8.  Implement the task.
9.  Run every gate and KEEP THE OUTPUT for the report:
      • cargo fmt --all --check
      • cargo clippy --workspace --all-targets -- -D warnings
      • cargo test --workspace
      • Anything named in the task's *Acceptance* section
        (e.g. cargo test -p rcc_lexer --test corpus).
10. On fully green, update state files AS PART of the commit:
      a. Flip the checkbox in the phase index.md: `[~]` → `[x]`.
      b. Prepend this banner at the very top of the task .md
         (above its `# ` heading), on its own line:
            > ✓ done — YYYY-MM-DD
      c. If that was the last `[ ]` in the phase, flip the phase
         line in tasks/index.md from `[ ]` to `[x]`.
      d. Run `git status --short` and `git diff --stat` — confirm
         only the files relevant to THIS task plus the state edits
         above are modified. Nothing else.
11. REPORT TO USER, then WAIT for approval.
    Post a single message containing, in order:
      A. Task id (e.g. `03-lex/05-pp-number`) + short title.
      B. `git status --short` output verbatim.
      C. The Acceptance checklist from the task file, with each
         item tick-marked (✓/✗) against your own execution.
      D. The full gate output summaries (one line per gate):
            fmt:     OK
            clippy:  OK
            test:    NN passed, 0 failed
            task:    <custom gate>  OK
      E. The proposed commit message (full, multi-paragraph —
         see "Commit message format" below). Fenced in a code
         block so the user can copy-paste unchanged.
      F. A single explicit question: "OK to commit?".
    STOP and wait. Do NOT proceed.
12. Interpret the user reply:
      • "ok" / "yes" / "go" / "commit" / Korean equivalents
        ("좋아" / "커밋해" / "ㅇㅋ") → proceed to step 13.
      • Any other reply (including requested changes) → treat as a
        revision request. Apply the changes, re-run step 9 gates,
        loop back to step 11 with the fresh output.
13. Create the commit:
      a. `git add -A`  (the state-file edits from step 10 are
         included in this same commit).
      b. Write the approved commit message to a temporary file
         and invoke:  `git commit -F <file>`
         (never use `git commit -m` — it mangles multi-paragraph
         messages on Windows shells).
      c. Print `git log -1 --oneline` so the SHA is visible.
14. STOP. One task, one commit, one session. Do not claim a second
    task in the same session even if time remains.
```

## Commit message format

The commit is the durable record of the task. It **must** follow
this shape verbatim:

```text
<phase>/<NN>: <imperative short title, <= 60 chars>

What:
- <bullet per code change>

Why:
- <link to C99 spec § or plan § as needed>
- <why this shape over alternatives, if there was a choice>

Implementation notes:
- <algorithm name, data structure choice>
- <edge cases explicitly handled>
- <deliberately deferred items with future task id>

Tests:
- <unit / UI / snapshot / conformance tests added>
- <gate results: fmt/clippy/test pass counts>

Acceptance (tasks/<phase>/<NN-name>.md):
- [x] <criterion 1>
- [x] <criterion 2>
- ...

Task: tasks/<phase>/<NN-name>.md
```

Rules for the message:

- Subject line: `<phase>/<NN>: <title>`, imperative mood, no period,
  ≤ 60 chars. Example: `03-lex/05: recognise C99 pp-numbers`.
- Blank line between subject and body (mandatory).
- Body lines wrap at 72 chars for `git log` readability.
- Bullets use `- ` (hyphen + space), never `*`.
- Every Acceptance checkbox from the task file appears in the final
  section with `[x]`; if any remained unchecked, you should not be
  committing yet — go back to step 7.
- The final `Task:` footer is the canonical link to the spec; tools
  like `git log --grep 'Task: tasks/03-lex/05-'` rely on it.

## State markers (canonical)

In any `index.md`:

| Marker | Meaning |
|--------|---------|
| `- [ ] 03-NN-name` | pending; first-in-order is "next". |
| `- [~] 03-NN-name` | in-progress; someone has claimed it. Skip. |
| `- [x] 03-NN-name` | done. Task file has `> ✓ done` banner; a commit exists. |

## Guardrails

- **One task per session, one commit per task.** Never batch two
  tasks into one commit; never split one task over two commits.
- **Never commit without explicit user approval.** Step 11 → 12 is
  the only path to step 13.
- **Never touch `.cursor/plans/*`.** Design is frozen there.
- **Never edit another task's checkbox.** You may only move your own
  claim forward (`[ ]` → `[~]` → `[x]`) or revert it (`[~]` → `[ ]`).
- **Never skip a pending task** to grab a later one. Dependencies are
  encoded by order.
- **Never amend a committed task.** Follow-up fixes are new tasks
  (or revert + re-do as a new task). `git commit --amend` is banned.
- **Do not add new xfail entries** unless the task explicitly permits
  it; existing xfails only shrink.
- **Do not read more files than the allowlist.** If you need more
  context, that means the task description is incomplete — record
  the gap under `## Notes (agent)` and revert the claim.

## Stuck / blocked / ambiguous

Do NOT commit in any of these cases. Revert the claim and report:

- **Compile error you cannot fix** → append 2–3 line
  `## Notes (agent)` diagnosis to the task file, revert
  `[~]` → `[ ]`, tell the user, stop.
- **Spec ambiguity** → note the exact C99 §-reference that is
  unclear, revert, stop.
- **Test suite fetch fails** → run `cargo xtask fetch-testsuites`
  once. If still broken, revert and report.
- **Upstream dep unexpectedly not `[x]`** → revert, tell the user
  which dep is missing, stop.

## Plan-level escalation

Some tasks cannot be completed because the **plan itself** (not the
task text, not a local spec ambiguity) is structurally infeasible
for the task's goal. Typical examples:

- The frozen public type boundary between two crates makes the
  Acceptance physically impossible.
- The chosen backend (`inkwell` via the `llvm` feature) cannot
  express an operation the task requires.
- The workspace topology forces a dependency cycle.
- Milestone ordering is wrong — the task depends on something
  scheduled *later* in the plan.

When you detect this, do the following:

1. Do NOT edit `.cursor/plans/*.plan.md` yourself. It is read-only
   for agents.
2. Revert your claim (`[~]` → `[ ]`) in the phase `index.md`.
3. Append a `## Plan-level concern (agent)` section at the bottom
   of the task file describing, concretely:
   - **Failure mode** — one paragraph, what breaks and why.
   - **Plan sections affected** — cite the `§`-numbers in the plan.
   - **Options** — 2–3 possible resolutions, one sentence each
     (e.g. "revise plan §4", "reshape this task", "swap the
     backend choice in plan §7").
4. REPORT TO USER with a single explicit question:
   **"Should the plan be revised, or should this task be reshaped?"**
5. STOP. Do not commit.

The user then either:

- Edits `.cursor/plans/*.plan.md` (agents still cannot touch it —
  only the human does) and updates any affected task files, OR
- Rewrites the affected task file to a reachable scope, OR
- Opens a `TCR-` task (see the working agreement) if the change
  is cross-cutting.

Resume only after the user hands back an updated task or says
"proceed as-is with <workaround>". Then start the cycle fresh
from step 1 of the one-cycle protocol.

## Where to look for deeper context (only if a task points there)

- Working agreement / commit rules: [`tasks/00-overview/03-working-agreement.md`](tasks/00-overview/03-working-agreement.md).
- KPI numbers per milestone: [`tasks/00-overview/02-kpi-dashboard.md`](tasks/00-overview/02-kpi-dashboard.md).
- Glossary of terms: [`tasks/00-overview/04-glossary.md`](tasks/00-overview/04-glossary.md).
- Phase dependency DAG: [`tasks/00-overview/01-phase-ordering.md`](tasks/00-overview/01-phase-ordering.md).

These are NOT part of the default reading list. Open them only when
a task's *References* explicitly links to one.

## TL;DR

Read `tasks/index.md` → active phase → its `index.md` → first `[ ]`
task → flip to `[~]`. Read task. Check deps. Failing tests first.
Implement. Run gates. Flip to `[x]`. Banner the task file. **Report
to user with proposed commit message. Wait for "OK". Then `git add
-A && git commit -F <file>`. Stop.**
