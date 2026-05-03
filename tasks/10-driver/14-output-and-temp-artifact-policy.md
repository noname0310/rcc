# 10-14: Output and temporary artifact policy

**Phase:** 10-driver    **Depends on:** 10-01, 10-02, 10-07    **Milestone:** M4    **Size:** Medium

## Goal

Make driver outputs deterministic and build-system friendly: default names,
multi-stage artifacts, temporary directories, cleanup, and `--save-temps`
should follow one explicit policy.

## Scope

- In:
  - Default output names for executable/object/assembly/IR/preprocessed output.
  - Multi-emit naming (`foo.tokens`, `foo.ast`, `foo.ll`, `foo.o`, etc.).
  - Private temp directory lifecycle for object/link intermediates.
  - `--save-temps` and `--save-temps=<dir>` behavior.
  - Collision handling when an output path equals an input path.
- Out:
  - Incremental compilation caches.
  - Build-system dependency files (`10-18`).

## Deliverables

- Central `OutputPlan` / `ArtifactPlan` data structure used by `pipeline.rs`.
- Tests for single input, explicit `-o`, stop flags, multi-emit, and
  `--save-temps`.
- Cleanup test proving failed links do not leave unnamed temp files unless
  `--save-temps` is active.

## Acceptance

- `rcc -c src/hello.c` writes `hello.o`.
- `rcc -S src/hello.c -o build/hello.s` writes exactly that path.
- `rcc hello.c --emit=llvm-ir --emit=mir -o build/out` writes deterministic
  stage files without clobbering `build/out`.
- `--save-temps=<dir>` preserves `.i`, `.ll`, `.s`, and `.o` intermediates.

## References

- GCC overall output file conventions
- Clang `-save-temps`
