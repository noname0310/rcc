> ✓ done — 2026-05-05

# 13-02: Diagnostic quality sweep

**Phase:** 13-quality    **Depends on:** 02-02    **Milestone:** M7

## Goal
Walk the entire error-code registry (`docs/error-codes.md`) and make every
user-facing diagnostic stable enough for UI tests and release notes.

Each emitted code must satisfy:
- Primary label highlights the offending source span.
- Secondary label illustrates the relevant context when one exists.
- `help:` is present when the user can reasonably fix the problem.
- Extension warnings name the flag that suppresses or enables the behavior.
- Internal invariant diagnostics explain which upstream phase should have
  caught the bug.

## Scope
- In:
  - Add or update UI snapshots for representative diagnostics from lexer,
    preprocessor, parser, HIR lower, typeck, CFG, codegen, and driver.
  - Ensure every W code includes the visible `-W...` or `-f...` control where
    applicable.
  - Ensure unknown or unsupported pragmas do not render as hard errors unless
    warning control promotes them.
- Out:
  - i18n.
  - Rewriting the diagnostic renderer.

## Deliverables
- Updated UI snapshots.
- `docs/error-codes.md` examples matched to actual emitted text.
- A small checklist in `docs/diagnostic-quality.md`.

## Acceptance
- Rubric checklist applied to every E code.
- Every code documented in `docs/error-codes.md` appears in either a unit test,
  UI test, or an explicit "not yet emitted" table.
- `cargo xtask check-error-codes` passes.
- `cargo test -p rcc_driver --test ui` passes.

## References
- rustc's "excellent diagnostics" convention.
