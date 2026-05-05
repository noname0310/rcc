# 03 — zlib probe plan

## Source snapshot

- Project: zlib
- Upstream URL: <https://github.com/madler/zlib>
- Clone command: `git clone --depth 1 https://github.com/madler/zlib.git upstream`
- Resolved commit: `f9dd6009be3ed32415edf1e89d1bc38380ecb95d`
- Date fetched: 2026-05-05

## Upstream build entry points

- Build files inspected:
  - `Makefile.in`
  - `Makefile`
  - `CMakeLists.txt`
  - `test/example.c`
  - `zlib.h`
  - `zconf.h`
- Smallest compile target:
  - core compression/decompression objects:
    `adler32.c`, `crc32.c`, `deflate.c`, `infback.c`, `inffast.c`,
    `inflate.c`, `inftrees.c`, `trees.c`, `zutil.c`, `compress.c`,
    `uncompr.c`
  - generated `scratch/zlib_smoke.c`
- Smallest runnable target: generated compress/uncompress one-buffer smoke.
- Required generated config: none. The upstream clone already contains
  `zconf.h`; the probe does not run `configure` and does not edit generated
  configure output.

## Baseline oracle

- Host compiler: `gcc`
- Host compile command:
  `gcc -std=c99 -Wall <core-sources> ../scratch/zlib_smoke.c -I. -o ../build/host/zlib_smoke`
- Host run command:
  `../build/host/zlib_smoke > ../artifacts/host-zlib-smoke.stdout`
- Expected exit status: `0`
- Expected stdout/stderr:
  - stdout: `zlib smoke ok`
  - stderr: empty

## rcc probe

- `rcc` compile command:
  `LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 RCC_LINKER_DRIVER=clang-18 target/release/rcc --std=c99 -Wall <core-sources> ../scratch/zlib_smoke.c -I. -o ../build/rcc/zlib_smoke`
- Link command, if separate: owned by `rcc`; wrapper selects `clang-18`.
- Run command:
  `../build/rcc/zlib_smoke > ../artifacts/rcc-zlib-smoke.stdout`
- Expected comparison: stdout must match the host compiler baseline exactly.

## Allowed local adaptation

- Wrapper scripts:
  - `scripts/run-smoke.sh`
- Build-script-only patches: none
- Generated config files: none
- Generated probe source:
  - `scratch/zlib_smoke.c` is created by the wrapper and ignored by git.

## Disallowed adaptation checklist

- [x] No upstream `.c` file modified
- [x] No upstream `.h` file modified
- [x] No failing upstream test deleted
- [x] No runtime oracle weakened to hide an `rcc` bug

## Failure log

| ID | Command | Symptom | Classification | Follow-up task |
| --- | --- | --- | --- | --- |
| ZLIB-001 | `rcc ... infback.c` | function-like macro invocation split across physical lines leaked unexpanded tokens | preprocessor bug | `tasks/04-preprocess/22-multiline-function-macro-invocation.md` |
| ZLIB-002 | `rcc --emit=llvm-ir adler32.c` | external incomplete array globals were rejected before LLVM codegen | codegen global declaration bug | `tasks/09-codegen-llvm/30-external-incomplete-array-globals.md` |
| ZLIB-003 | `rcc --emit=mir infback.c` | CFG panicked on `Index(ArrayToPtr(StringRef), ...)` | CFG lvalue place bug | `tasks/08-cfg/28-string-literal-index-place.md` |
| ZLIB-004 | `rcc --emit=hir zutil.c` | `(z_const char *)"..."` global initializer leaves stayed as `GlobalInitValue::Error` | typeck const-eval bug | `tasks/07-typeck/24-casted-string-global-initializer.md` |

## Exit criteria

- [x] Host baseline built
- [x] Host baseline run recorded, when applicable
- [x] `rcc` build attempted
- [x] `rcc` run compared with baseline, when applicable
- [x] Compiler bugs have minimized regressions
- [x] `RESULTS.md` updated
