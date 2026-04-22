> ✓ done — 2026-04-23

# 01-07: Pin every manifest rev to a commit SHA

**Phase:** 01-test-infra    **Depends on:** 01-01 .. 01-06    **Milestone:** M0.5

## Goal
Replace every `rev = "master" | "main" | "releases/..."` in
`third_party/MANIFEST.toml` with a **full 40-character commit SHA**.
Branch names are fine while prototyping but cause flake the moment
upstream force-pushes. This task is the "commit-gate" for everything
else in phase 01.

## Scope
- In: edit `MANIFEST.toml`, record the SHA picked + short description
  in the commit body.
- Out: bumping revs later (separate task each time).

## Deliverables
- All 6 suites use 40-char SHAs.
- `cargo xtask show-manifest` output in a CI log archive (proof of
  repro for reviewers).

## Acceptance
- `toml` parse of `MANIFEST.toml` shows every `rev` matches
  `^[0-9a-f]{40}$`.
- Fetch succeeds end-to-end for all permissive suites (GPL opt-in
  likewise on a `--include-gpl` CI job).
- `docs/conformance.md` regenerated; any nonzero `Discovered` column
  is plausible (no silent zero because a sparse path broke).

## References
- Plan §9.2 "xtask 크레이트 ... 재현성 보장".
