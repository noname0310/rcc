> ✓ done — 2026-05-05

# 15-07: System header search path discovery

**Phase:** 15-builtin-rt    **Depends on:** 15-02    **Milestone:** M6

## Goal
Automatically discover and configure default system include paths
so that `#include <stdio.h>` works without manual `-I` flags.
Implement `--sysroot` to override the root directory.

This task is about header discovery and declaration visibility, not
implementing libc. Hosted function bodies such as `printf`, `malloc`,
and `fopen` remain the responsibility of the target platform's libc/CRT
and the linker-driver configuration.

## Scope
- In: platform-specific search:
  - Linux: `/usr/include`, `/usr/local/include`,
    `/usr/include/<triple>`.
  - macOS: Xcode.app or CommandLineTools SDK paths via
    `xcrun --show-sdk-path`.
  - Windows: MSVC include dirs via `vswhere` / registry, or
    MinGW include dirs.
  `--sysroot <dir>` flag to prefix system paths.
  Search order: compiler-provided headers (from 15-02) first,
  then system headers.
- Out: C++ header search, framework search paths, hosted libc/glibc/MSVCRT
  implementation, and copying a platform libc into the repository.

## Deliverables
- System include path discovery module.
- `--sysroot` CLI flag.
- `-isystem <dir>` CLI flag for additional system include dirs.
- Test: on current host, `#include <stdio.h>` resolves.

## Acceptance
- On Linux, `rcc hello.c` (where hello.c includes `<stdio.h>`)
  finds the system `stdio.h` without explicit `-I`.
- The same hello-world links by using the platform libc/CRT through the
  driver/linker path; rcc does not provide `printf`.
- `--sysroot /custom/root` prepends the custom root to system
  include paths.
- Compiler-provided headers take priority over system headers.

## References
- GCC directory search order documentation.
- Clang `InitHeaderSearch.cpp`.
