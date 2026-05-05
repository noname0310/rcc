# 09 -- GNU coreutils

Status: cloned, plan pending execution

Source: <https://github.com/coreutils/coreutils>

Upstream commit: `d719413160b48136e3c3d6e5822f241eabddafb9`

This is the first intentionally glibc-heavy hosted Linux probe. The goal is not
to make `rcc` implement glibc; the host system still supplies glibc, POSIX
runtime libraries, and the linker. The compiler work is to parse and lower the
headers, generated gnulib configuration surface, and GNU userland C code
without papering over compiler bugs.

Do not edit files under `upstream/`. Any adaptation must live in this directory
as wrapper scripts, generated build logs, or build-script-only patches.

Initial target: bootstrap/configure with the host toolchain to generate
`lib/config.h`, then use `rcc` on one small utility translation unit before
expanding to more of `src/`.
