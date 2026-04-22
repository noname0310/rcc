# 01-11: Render `docs/conformance.md` dashboard

**Phase:** 01-test-infra    **Depends on:** 01-10    **Milestone:** M0.5

## Goal
Add a renderer that converts `docs/conformance.json` into a markdown
table inside `docs/conformance.md`. The renderer preserves hand-written
intro text (above a fenced `<!-- BEGIN autogen -->` marker) and only
rewrites the autogen block.

## Scope
- In: `crates/rcc_conformance/src/bin/cc_conformance_render.rs`;
  `<!-- BEGIN autogen -->` / `<!-- END autogen -->` sentinel handling;
  percentages computed as `(pass + xfail) / discovered` with single
  decimal place.
- Out: auto-commit workflow (that's a CI concern in task 13).

## Deliverables
- Binary that reads JSON + rewrites the autogen block.
- Unit test with a fixture JSON + expected markdown.

## Acceptance
- Running the binary on a real JSON leaves intro text untouched and
  updates the Suite-status table with current numbers.
- `cargo test -p rcc_conformance --test render` green.

## References
- Task 00-02 KPI matrix contract.
