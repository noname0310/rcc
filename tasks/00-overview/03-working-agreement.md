# 00-03: Working agreement

**Phase:** 00-overview    **Depends on:** —    **Milestone:** M0.5

## Goal
One page every agent (and human reviewer) internalises before running
the cycle in `agent.md`. Sets the conventions so commit reviews don't
bounce on trivial issues.

## Rules

1. **Tests first, code second.** Every task file lists acceptance
   criteria; translate them into failing `#[test]`s before editing
   the real code. A task that commits without a new test for new
   behaviour is rejected on review.
2. **One task → one commit → one session.** Never batch two tasks
   into one commit; never split one task across two commits. The
   state-file edits (index.md checkbox + task.md banner) travel in
   the same commit as the code changes for that task.
3. **Never break the skeleton's public types.** The crate-level
   public types were frozen in the M0–M3 skeleton. If a task needs
   to change a type that multiple crates depend on, raise a **Type
   Change Request**: open a task file under the affected phase with
   prefix `TCR-` and get human sign-off before touching code.
4. **Commit only after user approval.** The `agent.md` cycle ends
   at step 11 with a report and a question "OK to commit?". No
   commit happens until the user replies with an explicit go.
5. **Red gates = no report.** `cargo fmt`, `cargo clippy -D warnings`,
   `cargo test --workspace`, plus any task-specific gate listed under
   *Acceptance*, must all be green **before** the agent reports.
   If a gate is red, the agent either fixes it or reverts the claim.
6. **xfail is not defeat.** If a test case exposes a legitimate gap
   that a later milestone covers, add an entry to the suite's
   `xfail.toml` with a reason pointing at the future task id.
7. **Touch `docs/conformance.md` only through the runner.** Hand
   edits are wiped by the next `cargo conformance` invocation.
8. **Plan file is read-only.** The plan at
   `.cursor/plans/c_compiler_architecture_plan_*.plan.md` is the
   canonical design record. Task files can *reference* it but
   never mutate it.
9. **Task marking.** When acceptance is met and the commit lands,
   prepend `> ✓ done — YYYY-MM-DD` as the first line of the task
   file (above its `# ` heading). A `rg '^> ✓ done'` against the
   repo lists every shipped task. The git commit SHA is the
   authoritative cross-reference — find it with
   `git log --grep 'Task: tasks/<phase>/<NN-name>.md'`.
10. **`git commit --amend` is banned.** A task is done or not done.
    Follow-ups are new tasks. `git revert` is the only way to undo
    a shipped task.
11. **Plan infeasibility escalates to the user.** If a task cannot
    be completed because the plan itself is structurally wrong (not
    a local ambiguity — see rule 8), the agent follows the
    `Plan-level escalation` section of `agent.md`: append a
    `## Plan-level concern (agent)` block to the task file, revert
    the claim, report the options to the user, and stop. Only the
    user edits `.cursor/plans/*`.

## Commit message contract

See `agent.md` section **Commit message format** for the exact
template. The non-negotiable pieces:

- Subject starts with `<phase>/<NN>: ` (e.g. `03-lex/05: …`).
- Body has sections **What / Why / Implementation notes / Tests /
  Acceptance / Task:** in that order.
- Every Acceptance bullet from the task file is re-listed with `[x]`.

## Escalation

- Spec ambiguity → add `## Notes (agent)` to the task file citing
  the exact C99 paragraph, revert the claim, tell the user. A human
  then either decides in a comment inside the task file or opens a
  separate `TCR-` task.
- Cross-cutting refactor → open a `TCR-` task (see rule 3).
- Broken upstream test suite revision → bump the pin in
  `third_party/MANIFEST.toml`, record the reason in the commit body
  (since bumping the pin is itself a task), re-run
  `cargo xtask fetch-testsuites`, verify nothing else regresses.

## References
- Plan §8 진단 & 테스트 인프라.
- `docs/testing.md`.
- `agent.md` at the repo root (the cycle + commit template).
