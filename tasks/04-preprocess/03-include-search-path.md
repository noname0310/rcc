> ✓ done — 2026-04-23

# 04-03: `#include` search path

**Phase:** 04-preprocess    **Depends on:** 04-02, 03-09    **Milestone:** M5

## Goal
Implement C99 §6.10.2 header search. `"..."` form searches the
current file's directory first, then `Session::opts.include_paths`.
`<...>` form searches only the include paths.

## Scope
- In: `resolve_header(name: &str, system: bool, current_dir: &Path)
  -> Option<PathBuf>`; feed the result back into
  `Session::source_map::load_file`; invoke the preprocessor recursively
  on the new `FileId`.
- Out: include guards / `#pragma once` (tasks 04 and 05).

## Deliverables
- `resolve_header` + integration in directive handler.
- Unit test using a mock file system (create temp dirs).
- Diagnostic E0021 "cannot find header".

## Acceptance
- Fixture project with `main.c` including `"util.h"` in the same
  directory: resolves and includes once.
- `<stddef.h>` with no matching include path fails with E0021
  pointing at the `#include` line.

## References
- C99 §6.10.2.

## Notes (agent)
- Signature widened: `resolve_header` takes `&[PathBuf]` alongside
  `current_dir` so it is callable as a pure function in unit tests
  without a `Session`. `Preprocessor::process_include` wraps it and
  performs the `Session::source_map::load_file` + recursive
  preprocessing step the task spec describes.
- `Directive::Include::header` retains its delimiters (`<foo.h>` /
  `"foo.h"`). A helper `strip_header_delimiters` normalises those
  into the bare filename that `resolve_header` accepts; this keeps
  task 04-02's raw-substring contract unchanged while giving the
  resolver a clean input.
- **E0021 slot**: the lexer/preprocessor block E0001..E0020 is
  already fully allocated (see `## Notes (agent)` in 04-02). The
  task spec names the code E0021, which lives in the parser-reserved
  window. Registered it there and annotated `codes.rs` so future
  parser tasks start from E0022.
- Recursion currently goes through the `run()` pass-through stub;
  when tasks 04-06..04-14 replace `run` with a real directive loop
  the recursion picks up proper macro / conditional handling with no
  change to `process_include`.
