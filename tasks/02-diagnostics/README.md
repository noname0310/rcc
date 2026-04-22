# 02-diagnostics

**Goal of the phase.** Upgrade `rcc_errors::StderrEmitter` from the
current "one line per field" placeholder to a production-grade
experience: colour, carets, labels, multi-file contexts, stable error
codes, tests.

## Tasks

| # | File | Summary |
|---|------|---------|
| 01 | [`01-ariadne-emitter.md`](01-ariadne-emitter.md) | Real emitter backed by `ariadne`. |
| 02 | [`02-error-codes-registry.md`](02-error-codes-registry.md) | Stable `E0001..` registry + doc page. |
| 03 | [`03-multi-file-rendering.md`](03-multi-file-rendering.md) | Diagnostics that span two files (e.g. include chain). |
| 04 | [`04-capture-emitter-tests.md`](04-capture-emitter-tests.md) | Expand `CaptureEmitter` usage across the workspace. |

## Exit criteria

- `cargo run --bin rcc -- samples/bad.c --emit=checked` produces
  a user-readable, colourised error.
- Error codes appear in the online index doc `docs/error-codes.md`.
