# GNU coreutils probe plan

## Snapshot

- Upstream: <https://github.com/coreutils/coreutils>
- Commit: `d719413160b48136e3c3d6e5822f241eabddafb9`
- Local source: `real_world/projects/09-gnu-coreutils/upstream/`
- Source policy: never modify upstream `.c`, `.h`, `configure.ac`, `Makefile.am`,
  or generated files in place.

## Why This Project

GNU coreutils is a deliberately hard hosted-Linux target:

- It depends on glibc/POSIX/GNU headers through gnulib.
- It uses feature-test macros and generated `config.h` heavily.
- It exercises file-system, locale, getopt, signal, stat, dirent, fcntl, time,
  and process-related declarations.
- It is still mostly C99-compatible at the language level once the hosted
  environment is correctly modeled.

## Probe Order

1. Run the project's bootstrap/configure path with the host compiler in a local
   build directory and record the generated include paths and `config.h`
   surface.
2. Pick one small utility, starting with `src/true.c` because it is simple but
   still includes `config.h`, `system.h`, `<stdio.h>`, and `<sys/types.h>`.
3. Compile the selected translation unit with `rcc` using the same include
   order and feature macros as the host build.
4. If compilation fails because `rcc` is wrong or incomplete, stop the project
   probe and create or execute the corresponding compiler task under
   `tasks/16-linux-glibc-compat/`.
5. Once the first utility links and runs, compare a small runtime command
   against the host-built utility.

## Non-Goals

- Do not vendor glibc, gnulib, or Linux kernel headers into `rcc`.
- Do not patch coreutils source to avoid compiler bugs.
- Do not count a weakened probe as success. Every workaround must be either a
  build-script-only adaptation or a compiler task.

## Expected First Blockers

- Generated `config.h` and gnulib replacement-header include order.
- glibc-specific macro spellings such as `__THROW`, `__wur`, `__nonnull`,
  `__attribute_malloc__`, and feature-test conditionals.
- POSIX file-system declarations from `sys/types.h`, `sys/stat.h`, `fcntl.h`,
  `dirent.h`, and `unistd.h`.
- Linker/runtime flags for libc-provided functions, not rcc-owned function
  bodies.
