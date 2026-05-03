# 10-17: Response files

> ✓ done — 2026-05-04 — implemented in commit

**Phase:** 10-driver    **Depends on:** 10-12, 10-16    **Milestone:** M5    **Size:** Medium

## Goal

Support `@file` response files so `rcc` can consume build-system generated
command lines without hitting shell length limits.

## Scope

- In:
  - Recursive but cycle-detected `@file` expansion before clap parsing.
  - Shell-like quoting for spaces, backslashes, and comments.
  - UTF-8 error diagnostics.
  - Tests for nesting, cycle detection, Windows paths, and escaped quotes.
- Out:
  - MSVC-specific response file dialect quirks beyond the common subset.

## Deliverables

- Argument expansion layer before `Cli::parse_from`.
- `@file` UI tests.
- Clear diagnostics that include the response file path and line/column.

## Acceptance

- `rcc @args.rsp` behaves the same as passing the contained args directly.
- Cyclic response files fail with a usage-failure exit code.
- A missing response file produces a deterministic diagnostic.

## References

- Clang response file support
- GCC response file support
