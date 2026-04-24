# 10-11: Multi-file compilation

**Phase:** 10-driver    **Depends on:** 10-02    **Milestone:** M5

## Goal
Accept multiple `.c` input files on the command line. Compile each
to a temporary `.o` file, then link all object files together in a
single link step.

## Scope
- In: change `Cli::input` from `PathBuf` to `Vec<PathBuf>`.
  For each input file, run the full pipeline (preprocess → parse →
  lower → typeck → cfg → codegen) producing a temporary object
  file. After all files are compiled, invoke the linker with all
  object files. If `-c` is specified, produce one `.o` per input
  (no link step). Error handling: if any file fails, report errors
  but continue compiling remaining files, then exit with failure.
- Out: parallel compilation (future optimisation).

## Deliverables
- `Vec<PathBuf>` input in CLI.
- Per-file compilation loop.
- Multi-object link invocation.
- Tests: compile and link two `.c` files that reference each other.

## Acceptance
- `rcc main.c util.c -o prog` compiles both files and links them.
- `rcc -c main.c util.c` produces `main.o` and `util.o`.
- Error in `util.c` does not prevent `main.c` from compiling.

## References
- GCC multi-file compilation behaviour.
