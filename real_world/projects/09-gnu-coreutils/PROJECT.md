# 09 -- GNU coreutils

Status: bootstrap/configure scripted; generated config.h observed; true.c probe scripted

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

Probe entrypoint:

```sh
bash real_world/projects/09-gnu-coreutils/scripts/prepare-local-bootstrap-tools.sh
bash real_world/projects/09-gnu-coreutils/scripts/run-gnulib-config-probe.sh
bash real_world/projects/09-gnu-coreutils/scripts/run-true-probe.sh
```

The scripts keep cloned worktrees, generated headers, wrapper sources, and logs
under ignored `build/`, `scratch/`, and `logs/` directories.  They must not edit
files under `upstream/`.

Current `src/true.c` status: `run-true-probe.sh` is repeatable and reaches
generated gnulib replacement headers.  Remaining compiler-owned blockers are
tracked by tasks 16-21 through 16-24, starting with GNU `#include_next`.
