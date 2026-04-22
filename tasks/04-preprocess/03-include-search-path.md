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
