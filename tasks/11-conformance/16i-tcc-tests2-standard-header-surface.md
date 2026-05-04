# 11-16i: tcc-tests2 standard header surface

**Phase:** 11-conformance    **Depends on:** 11-16    **Milestone:** M6

## Goal
Provide enough host-standard-header surface for tests that include common C99
library headers without implementing libc.

## Scope
- In: `tcc-tests2::24_math_library` and `tcc-tests2::46_grep`.
- Out: reimplementing `glibc`, `musl`, or any full C standard library.

## Deliverables
- A policy note explaining how rcc uses host libc headers/libs while owning
  parser/typeck compatibility for the declarations it consumes.
- Driver/header-search changes or shim headers for the minimal C99 declarations
  needed by these fixtures.
- Linker flag handling for libm if the existing driver does not already expose
  it cleanly.

## Acceptance
- `24_math_library` and `46_grep` compile, link, and pass through tcc-tests2
  on WSL.
- The implementation does not pretend to provide a full libc.

## References
- `target/wsl/tcc-tests2-16-final.json`
- C99 §7.4, §7.12.
