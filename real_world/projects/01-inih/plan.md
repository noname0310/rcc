# 01 — inih probe plan

## Source snapshot

- Project: inih
- Upstream URL: <https://github.com/benhoyt/inih>
- Clone command: `git clone --depth 1 https://github.com/benhoyt/inih.git upstream`
- Resolved commit: `577ae2dee1f0d9c2d11c7f10375c1715f3d6940c`
- Date fetched: 2026-05-05

## Upstream build entry points

- Build files inspected:
  - `meson.build`
  - `tests/meson.build`
  - `tests/unittest.sh`
  - `tests/runtest.sh`
- Smallest compile target: `ini.c + tests/unittest.c`
- Smallest runnable target: `tests/unittest.c` run from `upstream/tests`
- Required generated config: none

## Baseline oracle

- Host compiler: `gcc`
- Host compile command:
  `gcc -std=c99 -Wall ../ini.c unittest.c -o ../../build/host/unittest_multi`
- Host run command:
  `../../build/host/unittest_multi > ../../artifacts/host-unittest-multi.stdout`
- Expected exit status: `0`
- Expected stdout/stderr: stdout must match `tests/baseline_multi.txt`; stderr is empty.

## rcc probe

- `rcc` compile command:
  `LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 RCC_LINKER_DRIVER=clang-18 target/release/rcc --std=c99 -Wall ../ini.c unittest.c -o ../../build/rcc/unittest_multi`
- Link command, if separate: owned by `rcc`; wrapper selects `clang-18` as the
  clang-compatible lld driver.
- Run command:
  `../../build/rcc/unittest_multi > ../../artifacts/rcc-unittest-multi.stdout`
- Expected comparison: stdout must match `tests/baseline_multi.txt` exactly.

## Allowed local adaptation

- Wrapper scripts:
  - `scripts/run-unittest-multi.sh`
- Build-script-only patches: none
- Generated config files: none

## Disallowed adaptation checklist

- [x] No upstream `.c` file modified
- [x] No upstream `.h` file modified
- [x] No failing upstream test deleted
- [x] No runtime oracle weakened to hide an `rcc` bug

## Failure log

| ID | Command | Symptom | Classification | Follow-up task |
| --- | --- | --- | --- | --- |
| INIH-001 | `rcc --std=c99 -Wall ../ini.c unittest.c` | Could not find `bits/wordsize.h`, `bits/timesize.h`, `sys/cdefs.h`, `gnu/stubs.h` from glibc headers | Linux GNU multiarch system include discovery gap | `tasks/15-builtin-rt/09-linux-multiarch-include-discovery.md` |
| INIH-002 | same command after adding `--isystem /usr/include/x86_64-linux-gnu` | `isspace` undeclared from `<ctype.h>` | incomplete compiler-provided C99 `<ctype.h>` hosted declaration shim | `tasks/15-builtin-rt/10-ctype-hosted-declarations.md` |
| INIH-003 | same command with fixed headers | `RCC_LINKER_DRIVER=clang` not found in this WSL image | local tool spelling; `clang-18` is installed | wrapper defaults to `clang-18` |

## Exit criteria

- [x] Host baseline built
- [x] Host baseline run recorded, when applicable
- [x] `rcc` build attempted
- [x] `rcc` run compared with baseline, when applicable
- [x] Compiler bugs have minimized regressions
- [x] `RESULTS.md` updated

