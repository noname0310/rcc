> ✓ done — 2026-05-05

# 13-03f: `-Wunreachable-code`

**Phase:** 13-quality    **Depends on:** 13-03    **Milestone:** M7

## Goal
Warn for simple block-local statements after `return`, `break`, `continue`, or
`goto` when `-Wextra` or `-Wunreachable-code` is enabled.

## Scope
- In:
  - Detect syntactically unreachable statements within the same compound block.
  - Avoid duplicate cascades after the first unreachable statement in a run.
  - Suppress with `-Wno-unreachable-code`.
- Out:
  - Full CFG reachability and constant-condition analysis.

## Deliverables
- Parser/HIR/CFG-level warning pass and tests.
- Docs entry in `docs/warnings.md`.

## Acceptance
- `return 0; x = 1;` warns under `-Wextra`.
- Code in the other branch of `if` is not incorrectly warned.
- Dead code in preprocessor-disabled branches is not seen by the detector.
