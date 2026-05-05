# 04 — LibTomMath probe plan

## Source snapshot

- Project: LibTomMath
- Upstream URL: <https://github.com/libtom/libtommath>
- Clone command: `git clone --depth 1 https://github.com/libtom/libtommath.git upstream`
- Resolved commit: `ae40a87a920099a7d9d00979570e0c8d917a1fd7`
- Date fetched: 2026-05-05

## Upstream build entry points

- Build files inspected:
  - `makefile`
  - `CMakeLists.txt`
  - `sources.cmake`
  - `tommath.h`
  - `tommath_private.h`
  - `s_mp_rand_platform.c`
- Smallest compile target:
  - every top-level `mp_*.c` and `s_*.c` translation unit
  - generated `scratch/libtommath_smoke.c`
- Smallest runnable target: generated arithmetic smoke using `mp_read_radix`,
  `mp_mul`, and `mp_to_radix`.
- Required generated config: none. The probe does not run upstream build
  generators and does not edit upstream sources or headers.

## Baseline oracle

- Host compiler: `gcc`
- Host compile command:
  `gcc -std=c99 -Wall -Wextra -I. <mp/s sources> ../scratch/libtommath_smoke.c -o ../build/host/libtommath_smoke`
- Host run command:
  `../build/host/libtommath_smoke > ../artifacts/host-libtommath-smoke.stdout`
- Expected exit status: `0`
- Expected stdout:
  `12193263112482853211126352690`

## rcc probe

- `rcc` compile/link command:
  `LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 RCC_LINKER_DRIVER=clang-18 target/release/rcc -j 8 --std=c99 -Wall -I. <mp/s sources> ../scratch/libtommath_smoke.c -o ../build/rcc/libtommath_smoke`
- Run command:
  `../build/rcc/libtommath_smoke > ../artifacts/rcc-libtommath-smoke.stdout`
- Expected comparison: stdout must match the host compiler baseline exactly.
- Diagnostic probe:
  `scripts/run-tu-ir-smoke.sh` can compile all library translation units to
  LLVM IR for per-TU triage; the pass condition is the linked smoke script.

## Allowed local adaptation

- Wrapper scripts:
  - `scripts/run-smoke.sh`
  - `scripts/run-tu-ir-smoke.sh`
- Build-script-only patches: none
- Generated config files: none
- Generated probe source:
  - `scratch/libtommath_smoke.c` is created by the wrapper and ignored by git.

## Disallowed adaptation checklist

- [x] No upstream `.c` file modified
- [x] No upstream `.h` file modified
- [x] No failing upstream test deleted
- [x] No runtime oracle weakened to hide an `rcc` bug

## Failure log

| ID | Command | Symptom | Classification | Follow-up task |
| --- | --- | --- | --- | --- |
| LTM-001 | `rcc --emit=llvm-ir s_mp_rand_platform.c` | builtin `errno.h` shadowed host libc but lacked `EINTR` | hosted runtime header bug | `tasks/15-builtin-rt/19-posix-errno-constants.md` |
| LTM-002 | `rcc -j 8 ... -o libtommath_smoke` | serial multi-TU driver hit the 5-minute operational timeout before linking | driver scalability issue | `tasks/10-driver/19-parallel-multi-input-object-builds.md` |
| LTM-003 | `rcc -j 8 ... -o libtommath_smoke` at `-O0` | LibTomMath `MP_HAS(...)` constant false branches leaked disabled platform calls into the object file | CFG constant-condition pruning bug | `tasks/08-cfg/29-constant-condition-dead-branch-pruning.md` |

## Exit criteria

- [x] Host baseline built
- [x] Host baseline run recorded
- [x] `rcc` build attempted with multi-input `-j`
- [x] `rcc` run compared with baseline
- [x] Compiler bugs have minimized regressions
- [x] `RESULTS.md` updated
