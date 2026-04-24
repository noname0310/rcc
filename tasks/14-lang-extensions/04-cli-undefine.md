# 14-04: `-U NAME` CLI flag

**Phase:** 14-lang-extensions    **Depends on:** —    **Milestone:** M5

## Goal
Add the `-U NAME` command-line flag to undefine a macro. Process
`-U` flags after `-D` flags during preprocessor initialisation so
that `-DFOO -UFOO` results in `FOO` being undefined.

## Scope
- In: add `cli_undefines: Vec<String>` to `Options`, parse `-U`
  in the CLI argument handler, apply undefines after defines in
  preprocessor init.
- Out: interaction with `#undef` in source (already works).

## Deliverables
- CLI parsing for `-U`.
- `Options::cli_undefines` field.
- Preprocessor init applies undefines after defines.
- Test: `-DFOO -UFOO` → `#ifdef FOO` is false.

## Acceptance
- `rcc -DFOO=1 -UFOO file.c` compiles with `FOO` undefined.
- `-U` for a macro that was never defined is silently ignored.

## References
- GCC/Clang `-U` flag behaviour.
