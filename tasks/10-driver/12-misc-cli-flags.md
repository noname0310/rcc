# 10-12: Miscellaneous CLI flags

**Phase:** 10-driver    **Depends on:** —    **Milestone:** M6

## Goal
Implement commonly expected CLI flags for GCC compatibility:
`-v` (verbose), `-std=c99` (standard selection), and ignore common
`-f` flags with a warning.

## Scope
- In:
  - `-v`: print the search paths, compiler version, and each
    sub-command invoked (preprocessor, codegen, linker).
  - `-std=c99`: accept as the default standard. `-std=c11`,
    `-std=gnu99`, etc. are rejected with a clear "unsupported
    standard" error message.
  - `-f<flag>`: silently accept (with optional warning) common
    flags that do not affect rcc behaviour: `-fPIC`,
    `-fno-strict-aliasing`, `-fwrapv`, `-fstack-protector`,
    `-fno-common`, `-fvisibility=hidden`. Unknown `-f` flags
    produce a warning.
- Out: actual implementation of `-fPIC` codegen, stack protectors.

## Deliverables
- CLI parsing for `-v`, `-std=`, `-f` family.
- Verbose output implementation.
- Standard validation.
- Ignored-flag warning infrastructure.
- Tests for each flag category.

## Acceptance
- `rcc -v hello.c` prints version and commands to stderr.
- `rcc -std=c99 hello.c` compiles normally.
- `rcc -std=c11 hello.c` emits "unsupported standard" error.
- `rcc -fPIC hello.c` compiles with a note that the flag is ignored.

## References
- GCC overall options documentation.
