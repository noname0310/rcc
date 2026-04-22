# 11-10: Continuously shrink xfail lists

**Phase:** 11-conformance    **Depends on:** all feature phases    **Milestone:** M2 → M7

## Goal
Every milestone bump should *remove* more xfail entries than it
adds. Treat xfail growth as tech-debt and require justification in
the PR description.

## Scope
- In: `xtask xfail-report` prints a delta between two commits; used
  in PR templates.
- Out: individual xfail deletions (done by feature tasks).

## Deliverables
- `cargo xtask xfail-report HEAD~1..HEAD`.
- PR template that pastes the delta.

## Acceptance
- PR template exists.
- Running the report on a recent feature PR shows shrink, not growth.

## References
- Plan §8.5, §9.3.
