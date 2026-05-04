# 13-12: Documentation consistency sweep

**Phase:** 13-quality    **Depends on:** 13-11    **Milestone:** M7

## Goal
Make top-level docs match the implementation and task tree. By this point the
project has moved far beyond the original architecture skeleton, so stale
claims are release risks.

## Scope
- In:
  - Review `README.md`, `docs/architecture.md`, `docs/testing.md`,
    `docs/conformance.md`, task READMEs, and workflow docs.
  - Remove stale references to PR workflow, Slack, unsupported platforms,
    obsolete pass-rate targets, or old `cc_` naming.
  - Ensure every documented command runs or is explicitly marked as requiring
    LLVM/network/GPL/manual setup.
  - Cross-link release docs, CI docs, platform docs, conformance docs, and
    fuzzing docs.
- Out:
  - Rewriting the architecture plan file.

## Deliverables
- Updated docs and task README files.
- A `docs/release-checklist.md` with commands in execution order.

## Acceptance
- `rg` finds no stale project name or workflow references outside historical
  quotes.
- Every command in the release checklist has been run or marked manual with a
  reason.
- Docs distinguish required C99 conformance from exploratory GNU/C11 surfaces.

## References
- `agent.md`.
- `tasks/index.md`.
- `docs/`.
