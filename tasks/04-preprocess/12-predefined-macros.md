# 04-12: Predefined macros

**Phase:** 04-preprocess    **Depends on:** 04-06    **Milestone:** M5

## Goal
Pre-populate the `MacroTable` with the C99 §6.10.8 mandatory macros
plus a small pragmatic set:
- `__FILE__`, `__LINE__` (dynamic; expanded at use site).
- `__DATE__`, `__TIME__` (frozen at compile start).
- `__STDC__` = `1`, `__STDC_VERSION__` = `199901L`,
  `__STDC_HOSTED__` = `1`.
- `__func__` handled at the parser level, not here — document the
  split.

## Scope
- In: `Preprocessor::install_predefined()` called from `run()`;
  dynamic macros get a sentinel `MacroKind::Builtin` variant so
  expansion can special-case them.
- Out: CLI `-D` handling (already in `Options::cli_defines`; just
  wire it up here to install object-like macros first).

## Deliverables
- New `MacroKind::Builtin(BuiltinMacro)` variant.
- Tests asserting each macro is defined + expands to the correct
  shape (type checked later for literal value).

## Acceptance
- `__STDC_VERSION__` expands to `199901L`.
- `__LINE__` on line 42 expands to pp-number `42`.
- `__FILE__` expands to a string literal of the current file path.

## References
- C99 §6.10.8.
