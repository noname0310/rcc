# 02-04: Spread `CaptureEmitter` across the workspace

**Phase:** 02-diagnostics    **Depends on:** 02-01    **Milestone:** M1+

## Goal
Every crate that emits diagnostics (lexer, preprocess, parse, typeck)
gains a helper that builds a `Session` wired to a `CaptureEmitter`, so
unit tests can assert on structured diagnostics instead of stderr.

## Scope
- In: `rcc_session::Session::for_test()` constructor returning
  `(Session, CaptureEmitter)`; add it once, re-use everywhere.
- Out: individual test bodies (those live in the feature tasks).

## Deliverables
- `Session::for_test` with unit test proving it plumbs through.
- Update `crates/rcc_errors/tests/capture.rs` to use the helper (net
  code reduction expected).

## Acceptance
- `cargo test -p rcc_session for_test` green.
- No downstream crate needs to build its own Session test rig.

## References
- Plan §8.1.
