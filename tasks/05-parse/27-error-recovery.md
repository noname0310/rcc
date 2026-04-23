> ✓ done — 2026-04-24

# 05-27: Error recovery

**Phase:** 05-parse    **Depends on:** 05-13 .. 05-26    **Milestone:** M2

## Goal
On an unexpected token, emit a diagnostic and resynchronise by
skipping tokens until a statement-terminator (`;`) or block-closer
(`}`). Continue parsing; avoid cascades of downstream errors.

## Scope
- In: `Parser::recover_to_sync()` helper; call sites in `parse_stmt`,
  `parse_decl`, `parse_block`.
- Out: recovery inside expressions (too aggressive; better to fail
  the whole statement).

## Deliverables
- Recovery helper.
- Golden `.stderr` fixtures showing multi-error output (task 29).

## Acceptance
- Pathological fixture `tests/ui/bad_stmt.c` with 3 intentional
  syntax errors produces 3 distinct diagnostics.

## References
- rustc's `rustc_parse::parser::recover`.
