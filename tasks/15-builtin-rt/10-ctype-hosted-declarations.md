> ✓ done — 2026-05-05

# 15-10: Complete C99 ctype hosted declarations

**Phase:** 15-builtin-rt    **Depends on:** 15-04    **Milestone:** real-world-01

## Goal
Expose the C99 `<ctype.h>` classification/conversion function declarations in
the compiler-provided hosted header shim.

## Scope
- In: declarations for `isalnum`, `isalpha`, `isblank`, `iscntrl`, `isdigit`,
  `isgraph`, `islower`, `isprint`, `ispunct`, `isspace`, `isupper`,
  `isxdigit`, `tolower`, and `toupper`.
- Out: libc implementations and locale-specific behavior.

## Acceptance
- A builtin-runtime fixture including `<ctype.h>` compiles, links against host
  libc, runs, and verifies representative calls.

## Real-world trigger
`inih/ini.c` uses `isspace` through `<ctype.h>`. The previous shim exposed only
`tolower`, so type checking reported `isspace` as undeclared.

