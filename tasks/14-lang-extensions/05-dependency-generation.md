> ✓ done — 2026-05-05

# 14-05: Dependency file generation (`-M` / `-MM` / `-MF` / `-MT`)

**Phase:** 14-lang-extensions    **Depends on:** —    **Milestone:** M6

## Goal
Implement Makefile-format dependency output. Record every `#include`
resolved path during preprocessing and emit a `.d` file listing the
translation unit's dependencies.

## Scope
- In: `-M` (all headers), `-MM` (non-system headers only),
  `-MF <file>` (output path), `-MT <target>` (override target name
  in the rule). Collect paths in a `Vec<PathBuf>` during include
  resolution, write Makefile rule after preprocessing completes.
- Out: `-MD` / `-MMD` (depfile alongside normal compilation —
  combine with driver pipeline later).

## Deliverables
- CLI flags: `-M`, `-MM`, `-MF`, `-MT`.
- Dependency path collector in `rcc_preprocess`.
- Makefile-format `.d` file writer.
- Test: compile a file with `#include`, verify `.d` file contents.

## Acceptance
- `rcc -M -MF out.d file.c` produces `out.d` containing
  `file.o: file.c header1.h header2.h`.
- `-MM` excludes system headers (those found via `<>` in system
  include paths).
- Long lines are escaped with `\` continuation.

## References
- GCC `-M` family documentation.
