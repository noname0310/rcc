> ✓ done — 2026-04-24

# 05-29: Parser UI tests

**Phase:** 05-parse    **Depends on:** 05-27    **Milestone:** M2

## Goal
Golden `.stderr` fixtures for user-facing parse errors. Matches
rustc's `tests/ui` convention: `tests/ui/parse/*.c` + a sibling
`.stderr` file; a runner asserts stderr matches byte-for-byte.

## Scope
- In: 15-20 fixtures, one per error-recovery case in task 27 and
  per common mistake (missing `;`, mismatched `}`, bad declarator).
- Out: checked-in non-parser diagnostics (HIR / typeck).

## Deliverables
- Fixtures under `crates/rcc_driver/tests/ui/parse/`.
- A runner task (shared with other UI tests — see task
  [`10-driver/03-ui-test-harness.md`](../10-driver/03-ui-test-harness.md)).

## Acceptance
- `cargo test -p rcc_driver --test ui`: green.
- Regenerate with `UPDATE_EXPECT=1 cargo test` (convention).

## References
- rustc `tests/ui`.
