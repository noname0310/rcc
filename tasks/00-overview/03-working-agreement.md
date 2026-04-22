# 00-03: Working agreement

**Phase:** 00-overview    **Depends on:** —    **Milestone:** M0.5

## Goal
One page that every agent reads before starting any task. Sets the
conventions so PRs don't bounce on trivial issues.

## Rules

1. **Tests first, code second.** Every task file lists acceptance
   criteria; translate them into failing `#[test]`s before editing the
   real code. PRs without a test for new behaviour are rejected.
2. **Never break the skeleton's public types.** The crate-level public
   types were frozen in the M0-M3 skeleton. If a task needs to change
   a type that multiple crates depend on, raise a **Type Change
   Request** (open a task file under the affected phase with prefix
   `TCR-` and get sign-off before touching code).
3. **One task per PR.** Task files correspond 1-to-1 with PRs. Task
   id goes in the PR title: `lex/05: pp-number recogniser`.
4. **Red CI = blocked.** `cargo fmt`, `cargo clippy -D warnings`,
   `cargo test --workspace`, and the milestone-appropriate conformance
   subset must be green.
5. **xfail is not defeat.** If a test case exposes a legitimate gap
   that a later milestone covers, add an entry to the suite's
   `xfail.toml` with a reason pointing at the future task id.
6. **Touch `docs/conformance.md` only through the runner.** Hand-edits
   are wiped by the next `cargo conformance` invocation.
7. **Plan file is read-only.** The plan at
   `.cursor/plans/c_compiler_architecture_plan_*.plan.md` is the
   canonical design record. Task files can *reference* it but never
   mutate it.
8. **Task marking.** When acceptance is met, prepend `> ✓ done — <date> — <PR link>`
   to the task file's top so a `rg '^> ✓ done'` tells you what's
   shipped.

## Escalation

- Spec ambiguity → open an issue labelled `c99-spec` citing the exact
  paragraph.
- Cross-cutting refactor → open a `TCR-` task (see rule 2).
- Broken upstream test suite revision → bump the pin in
  `third_party/MANIFEST.toml`, record the reason in the commit body,
  re-run fetch-testsuites, verify nothing else regresses.

## References
- Plan §8 진단 & 테스트 인프라.
- `docs/testing.md`.
