> ✓ done — 2026-05-04

# 11-18: Continuously shrink xfail lists

**Phase:** 11-conformance    **Depends on:** all feature phases    **Milestone:** M2 → M7

## Goal
Every milestone bump should *remove* more xfail entries than it
adds. Treat xfail growth as tech-debt and require justification in
the commit message body.

## Scope
- In: `xtask xfail-report` prints a delta between two commits; used
  in commit message footers.
- Out: individual xfail deletions (done by feature tasks).

## Deliverables
- `cargo xtask xfail-report HEAD~1..HEAD`.
- commit message footer that pastes the delta.

## Acceptance
- commit message footer exists.
- Running the report on a recent feature commit shows shrink, not growth.

## References
- Plan §8.5, §9.3.
