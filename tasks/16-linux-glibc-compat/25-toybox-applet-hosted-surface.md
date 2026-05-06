# 16-25: Toybox Applet Hosted Surface

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-24-coreutils-true-runtime-oracle  
**Milestone:** hosted-linux

## Goal

Make the initial Toybox applet smoke compile and run with `rcc`, without
editing upstream Toybox `.c` or `.h` files and without weakening the runtime
oracle.

## Scope

- In: `real_world/projects/10-toybox/scripts/run-applet-smoke.sh`.
- In: compiler/header fixes required by Toybox's smallest applet set:
  `true false echo cat wc`.
- In: deciding whether `_Noreturn` belongs in language extension handling, a
  hosted header shim, or both.
- In: hosted Linux declarations/types/constants exposed by Toybox:
  `sigjmp_buf`, `timer_t`, `SIGKILL`, `SIGWINCH`, `stpcpy`, `syscall`,
  `netinet/tcp.h`, and duplicate `timespec`/`timeval` protection.
- Out: editing Toybox upstream source files.
- Out: deleting applets from the selected smoke set to hide a compiler bug.
- Out: implementing libc function bodies; host glibc supplies runtime bodies.

## Acceptance

- [ ] `bash real_world/projects/10-toybox/scripts/run-applet-smoke.sh` builds
      the selected applets with host `cc` and with `rcc`.
- [ ] The wrapper compares host and `rcc` exit status, stdout, and stderr for
      `true`, `false`, `echo`, `cat`, and `wc`.
- [ ] Any compiler bug fixed for this task has a minimized regression in the
      owning crate or driver e2e tests.
- [ ] `real_world/projects/10-toybox/RESULTS.md` records the passing command
      and observed output.
- [ ] `real_world/hosted-linux-dashboard.md` marks Toybox syntax/HIR, object,
      link, and runtime cells accurately.

## Current Failure

Command:

```sh
NO_COLOR=1 LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 \
  bash real_world/projects/10-toybox/scripts/run-applet-smoke.sh
```

Observed result:

- Host applet build succeeds.
- `rcc` stops while compiling the first applet source set.
- Diagnostics mention `_Noreturn`, `sigjmp_buf`, `timer_t`, `SIGKILL`,
  `SIGWINCH`, `stpcpy`, `syscall`, `netinet/tcp.h`, and duplicate
  `timespec`/`timeval`.

## Notes

The Toybox wrapper delegates stdin-only build probes (`-E -dM -`, library
detection snippets) to host `cc`. That is build-system adaptation, not target
source compilation. Ordinary applet object compilation and final linking must
go through `rcc`.
