> ✓ done — 2026-04-23

# 02-03: Multi-file diagnostic rendering

**Phase:** 02-diagnostics    **Depends on:** 02-01    **Milestone:** M5 prep

## Goal
When a diagnostic's labels span more than one `FileId`, the emitter
must render each file as its own ariadne `Report` group with a clear
header. The canonical example is a preprocessor error attached to the
`#include` site *and* the offending token in the included header.

## Scope
- In: extend `StderrEmitter::emit` to group labels by `FileId`, emit
  one `ariadne::Report` per file, print them back-to-back with a
  separator line.
- Out: notes that mention a span without rendering it (kept simple).

## Deliverables
- Snapshot test `tests/snapshots/render__multi_file.snap`.
- An `include-chain` helper in `rcc_errors` for the common
  "header A included from B included from main.c" note pattern.

## Acceptance
- Fixture diagnostic with 2 files renders two clearly separated
  blocks, each with its own source excerpt.
- No regression on single-file snapshot from task 01.

## References
- ariadne multi-source examples.
