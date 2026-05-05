# 02 — cJSON probe plan

## Source snapshot

- Project: cJSON
- Upstream URL: <https://github.com/DaveGamble/cJSON>
- Clone command: `git clone --depth 1 https://github.com/DaveGamble/cJSON.git upstream`
- Resolved commit: `fb16e5cf358798aabb049655975cde8427101056`
- Date fetched: 2026-05-05

## Upstream build entry points

- Build files inspected:
  - `Makefile`
  - `CMakeLists.txt`
  - `test.c`
  - `cJSON.h`
- Smallest compile target: `cJSON.c + generated roundtrip.c`
- Smallest runnable target: generated round-trip program under `scratch/`
- Required generated config: none

## Baseline oracle

- Host compiler: `gcc`
- Host compile command:
  `gcc -std=c99 -Wall cJSON.c ../scratch/roundtrip.c -I. -lm -o ../build/host/roundtrip`
- Host run command:
  `../build/host/roundtrip > ../artifacts/host-roundtrip.stdout`
- Expected exit status: `0`
- Expected stdout/stderr:
  - stdout: `{"name":"rcc","answer":42}`
  - stderr: empty

## rcc probe

- `rcc` compile command:
  `LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 RCC_LINKER_DRIVER=clang-18 target/release/rcc --std=c99 -Wall cJSON.c ../scratch/roundtrip.c -I. -lm -o ../build/rcc/roundtrip`
- Link command, if separate: owned by `rcc`; wrapper selects `clang-18`.
- Run command:
  `../build/rcc/roundtrip > ../artifacts/rcc-roundtrip.stdout`
- Expected comparison: stdout must match the host compiler baseline exactly.

## Allowed local adaptation

- Wrapper scripts:
  - `scripts/run-roundtrip.sh`
- Build-script-only patches: none
- Generated config files: none
- Generated probe source:
  - `scratch/roundtrip.c` is created by the wrapper and ignored by git.

## Disallowed adaptation checklist

- [x] No upstream `.c` file modified
- [x] No upstream `.h` file modified
- [x] No failing upstream test deleted
- [x] No runtime oracle weakened to hide an `rcc` bug

## Failure log

| ID | Command | Symptom | Classification | Follow-up task |
| --- | --- | --- | --- | --- |
| CJSON-001 | `rcc --std=c99 -Wall cJSON.c ../scratch/roundtrip.c -I. -lm` | `strtod` and `sscanf` undeclared, followed by call-expression errors | incomplete hosted declaration shims for numeric parsing APIs | `tasks/15-builtin-rt/12-hosted-core-declaration-sweep.md` |

## Exit criteria

- [x] Host baseline built
- [x] Host baseline run recorded, when applicable
- [x] `rcc` build attempted
- [x] `rcc` run compared with baseline, when applicable
- [x] Compiler bugs have minimized regressions
- [x] `RESULTS.md` updated
