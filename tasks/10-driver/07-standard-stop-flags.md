# 10-07: Standard stop flags (`-c`, `-S`, `-E`)

**Phase:** 10-driver    **Depends on:** 10-01    **Milestone:** M3

## Goal
Implement the standard GCC-compatible stop flags: `-c` (compile to
object file, do not link), `-S` (compile to assembly), `-E`
(preprocess only, output to stdout). Map these to the existing
`--emit=obj/asm/pp` internal representation.

## Scope
- In: CLI parsing for `-c`, `-S`, `-E`. `-c` sets emit mode to
  object and implies default output `<input>.o`. `-S` sets emit
  mode to assembly with default `<input>.s`. `-E` sets emit mode
  to preprocessed output, default to stdout. Mutual exclusivity
  check (only one stop flag allowed).
- Out: `-emit-llvm` modifier (future).

## Deliverables
- CLI flag parsing for `-c`, `-S`, `-E`.
- Default output filename logic.
- Mutual exclusivity validation with diagnostic.
- Tests: each flag produces the correct output type.

## Acceptance
- `rcc -c hello.c` produces `hello.o` (no link step).
- `rcc -S hello.c` produces `hello.s` (assembly text).
- `rcc -E hello.c` writes preprocessed output to stdout.
- `rcc -c -S hello.c` emits an error about conflicting flags.

## References
- GCC `-c`, `-S`, `-E` flag documentation.
