# 02-01: Ariadne-backed emitter

**Phase:** 02-diagnostics    **Depends on:** —    **Milestone:** M0.5

## Goal
Replace the placeholder `StderrEmitter` with an implementation that
uses the `ariadne` crate to render:
- Colourised severity tag (`error`, `warning`, ...).
- Source snippet with underline + caret under every `Label`.
- Multiple labels pointing to different spans.
- Attached notes/help lines after the snippet.

## Scope
- In: `crates/rcc_errors/src/emitter.rs` rewrite of `StderrEmitter`;
  integrate `ariadne::Report` / `Source`; pull file contents from
  `cc_span::SourceMap` via a small adapter trait.
- Out: multi-file rendering (task 03).

## Deliverables
- `StderrEmitter::new(sm: Arc<SourceMap>)` constructor.
- Tests in `crates/rcc_errors/tests/render.rs` using `insta` against a
  small fixture span / diagnostic.

## Acceptance
- A diagnostic with primary + secondary label produces a snapshot
  identical to `tests/snapshots/render__single_file.snap` (reviewed
  and checked in).
- ANSI colours disabled when stdout is not a TTY (confirmed via
  `NO_COLOR=1` env test).

## References
- `ariadne` crate docs.
- rustc's `rustc_errors` human emitter for layout prior art.
