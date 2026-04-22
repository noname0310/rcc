# 04-05: `#pragma once`

**Phase:** 04-preprocess    **Depends on:** 04-03    **Milestone:** M5

## Goal
Support the widely-used `#pragma once` non-standard extension: when
encountered, cache the current `FileId` in
`Preprocessor::pragma_once`; subsequent includes short-circuit.

## Scope
- In: record presence; respect on re-include; unit test.
- Out: other `#pragma` forms (task 16 handles `unknown pragma`
  diagnostics).

## Deliverables
- Handler in directive dispatcher.
- Fixture test: header with `#pragma once`, included twice from two
  different files; content expanded exactly once.

## Acceptance
- Second include is a no-op (verified via `emit=pp` byte count).
- Doesn't conflict with an explicit `#ifndef` guard in the same file.

## References
- Clang / GCC docs on `#pragma once`.
