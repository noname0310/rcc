# 10 -- Toybox

Status: initial applet smoke blocked by hosted Linux header/language gaps

Source: <https://github.com/landley/toybox>

Toybox is a Linux command-line utility suite with a Linux-kernel-style build
flow. It is the next glibc/POSIX stress target after the current hosted-header
work because it exercises ordinary Unix APIs across files, directories,
processes, signals, terminals, and command dispatch while still keeping the
runtime dependency model intentionally small.

Do not edit files under `upstream/`. Any adaptation must live in this directory
as wrapper scripts, generated config inputs, or build-script-only patches.

Initial target:

1. Clone upstream into `upstream/` and record the resolved commit in `plan.md`.
2. Build a host baseline for a tiny applet subset.
3. Compile the same selected translation units with `rcc`.
4. Run byte-for-byte runtime oracles for simple applets such as `true`, `false`,
   `echo`, `cat`, and `wc` before expanding to broader `defconfig` coverage.

Entry point:

```sh
bash real_world/projects/10-toybox/scripts/run-applet-smoke.sh
```

Current blocker: `tasks/16-linux-glibc-compat/25-toybox-applet-hosted-surface.md`.
