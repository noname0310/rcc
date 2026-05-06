# Toybox Results

Last verified: 2026-05-06 on WSL/Linux with LLVM 18.

## Applet Smoke

Command:

```sh
LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 \
  bash real_world/projects/10-toybox/scripts/run-applet-smoke.sh
```

Result: blocked.

- Upstream commit: `36fc372482bc4eb8b96e546b5337d03bef341dcf`
- Host baseline compiler: `cc`
- `rcc` target: `x86_64-unknown-linux-gnu`
- Applets covered:

```text
true
false
echo
cat
wc
```

The wrapper builds each selected applet twice through Toybox's own
`scripts/single.sh`: once with the host compiler and once with `rcc`. It then
compares exit status, stdout, and stderr for each applet.

The host baseline builds and runs. The `rcc` build currently stops while
compiling the first applet source set.

Observed blocker:

```text
lib/lib.h: _Noreturn declaration syntax is not accepted
lib/portability.c: stpcpy, syscall, timer_t, SIGKILL are missing
lib/tty.c: SIGWINCH is missing
toys.h: sigjmp_buf is missing
/usr/include/...: timespec/timeval redeclaration and netinet/tcp enum parsing gaps
```

## Compiler Bugs Found

| ID | Status | Symptom |
| --- | --- | --- |
| TBX-001 | open | Toybox's smallest applet build reaches hosted Linux header/language gaps before object emission. Tracked by `tasks/16-linux-glibc-compat/25-toybox-applet-hosted-surface.md`. |

## Upstream Source Policy

The wrapper does not modify upstream `.c` or `.h` files. The local `upstream/`
tree and build/log outputs are ignored by git.
