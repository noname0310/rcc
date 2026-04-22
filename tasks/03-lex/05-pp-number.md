# 03-05: pp-number recogniser

**Phase:** 03-lex    **Depends on:** 03-01    **Milestone:** M1

## Goal
Recognise C99 preprocessing numbers per §6.4.8. These are NOT fully
parsed integers or floats yet — a pp-number is just a maximal run
matching `pp-number := digit | .digit | pp-number (digit | identifier-nondigit | e± | E± | p± | P± | .)`.
Actual decoding happens in phase 05 (parser).

## Scope
- In: maximal-munch recogniser; classify as `PpNumberKind::Integer`
  when no `.`, `e`/`E`, `p`/`P` is present, otherwise `Float`; return
  the full span regardless of validity.
- Out: conversion to a numeric value (task 05-03).

## Deliverables
- Handler for `.42`, `0x1.0p0`, `3.14e-10f`, `0123`, `0xFFULL`.
- Table of fixtures covering every classification branch.

## Acceptance
- Lexing `3.14f + 0xdeadbeefULL` yields exactly two `PpNumber` tokens
  with the documented `PpNumberKind` classification.
- Fuzz corpus seeded with numeric literals yields no panic and the
  emitted spans partition the input bytes exactly.

## References
- C99 §6.4.8 preprocessing numbers.
