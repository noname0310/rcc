> ✓ done — 2026-05-06

# 15a-06: Anonymous Structs/Unions as Standard C11

**Phase:** 15a-c11-transition  
**Depends on:** 15a-01-language-standard-mode  
**Milestone:** c11-transition

## Goal

Make anonymous structs and unions part of the strict C11 path, not only a GNU
compatibility behavior, while keeping C99 diagnostics intact.

## Scope

- In: audit existing anonymous record parse/lowering/layout behavior.
- In: switch warnings or feature gates so `-std=c11` treats anonymous
  struct/union members as standard.
- In: keep C99 behavior explicit.
- In: add layout tests for nested anonymous records, duplicate promoted member
  names, and union member access.
- Out: Microsoft-only anonymous record extensions that C11 does not cover.

## Acceptance

- [x] `struct S { union { int x; long y; }; };` parses and lowers in C11 mode.
- [x] Member lookup through anonymous records works in typeck and codegen.
- [x] Duplicate ambiguous member names produce a diagnostic.
- [x] Existing GNU/MS bitfield behavior is unchanged.

## References

- N1570 6.7.2.1 structure and union specifiers.
- N1570 foreword "anonymous structures and unions" change note.
