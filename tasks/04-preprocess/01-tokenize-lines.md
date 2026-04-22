# 04-01: Tokenise by logical line

**Phase:** 04-preprocess    **Depends on:** 03-03    **Milestone:** M1+

## Goal
Wrap the pp-token stream in an iterator that groups tokens by logical
line (terminated by `Newline` and not preceded by `\\`). Directive
detection and expansion both operate on lines.

## Scope
- In: `LineStream` iterator yielding `Vec<PpToken>`; `#` is recognised
  only at `at_line_start` position.
- Out: directive parsing (task 02).

## Deliverables
- `crates/rcc_preprocess/src/line_stream.rs` with `LineStream::next_line`.
- Unit tests: `#include<x>`, `a = b ;\n`, blank lines.

## Acceptance
- Consecutive newlines yield empty lines (preserved so diagnostics
  report correct line numbers).
- Line containing only whitespace is still emitted (empty tokens
  vec) to keep `__LINE__` predictable.

## References
- C99 §6.10 preamble.
