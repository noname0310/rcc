# 05 - Lua probe plan

## Source snapshot

- Project: Lua
- Upstream URL: <https://www.lua.org/source/>
- Download URL: <https://www.lua.org/ftp/lua-5.5.0.tar.gz>
- Archive SHA256:
  `57ccc32bbbd005cab75bcc52444052535af691789dba2b9016d5c50640d68b3d`
- Date fetched: 2026-05-06

The source is unpacked into ignored `upstream/`. The wrapper never edits
upstream `.c` or `.h` files.

## Upstream build entry points

- Build files inspected:
  - top-level `Makefile`
  - `src/Makefile`
  - `src/luaconf.h`
  - `src/lvm.c`
  - `src/lstate.h`
  - `src/lobject.h`
- Smallest library target:
  - `CORE_O` from `src/Makefile`
  - `LIB_O` from `src/Makefile`
- Smallest runnable target:
  - `CORE_O + LIB_O + lua.c`

## Baseline oracle

- Host compiler: `gcc`
- Host flags:
  `-std=c99 -Wall -Wextra -DLUA_USE_JUMPTABLE=0 -DLUA_NOBUILTIN -I.`
- Link libraries: `-lm`
- Runtime command:
  `build/host/lua -e 'print(_VERSION); print(6*7)'`
- Expected stdout:

```text
Lua 5.5
42
```

## rcc probe

- `rcc` flags:
  `-j 8 --std=c99 -Wall -DLUA_USE_JUMPTABLE=0 -DLUA_NOBUILTIN -I.`
- Link libraries: `-lm`
- Linker driver:
  `RCC_LINKER_DRIVER=clang-18`
- Runtime command:
  `build/rcc/lua -e 'print(_VERSION); print(6*7)'`

The generic C99 Lua build is used first. `LUA_USE_LINUX` is intentionally left
off because it expands the first probe into `dlopen` and readline/platform
surface. `LUA_USE_JUMPTABLE=0` keeps the interpreter off GCC computed-goto
extensions so the probe remains a C99 target.

## Allowed local adaptation

- Wrapper scripts:
  - `scripts/run-smoke.sh`
- Generated files:
  - `scratch/`
  - `logs/`
  - `artifacts/`
  - `build/`
- Build flags:
  - `-DLUA_USE_JUMPTABLE=0`
  - `-DLUA_NOBUILTIN`

## Disallowed adaptation checklist

- [x] No upstream `.c` file modified
- [x] No upstream `.h` file modified
- [x] No generated Lua parser/runtime code disabled
- [x] No runtime oracle weakened to hide an `rcc` bug

## Failure log

| ID | Command | Symptom | Classification | Follow-up task |
| --- | --- | --- | --- | --- |
| LUA-001 | `rcc --emit=llvm-ir lfunc.c` and related TUs | fixed array bounds using enum constants, casts, or `offsetof` were misclassified as VLAs | HIR constant-expression lowering bug | `tasks/06-hir-lower/33-array-bound-ice-constants.md` |
| LUA-002 | `rcc ... loslib.c lua.c` | builtin `<stdlib.h>` lacked `EXIT_SUCCESS` and `EXIT_FAILURE` | hosted runtime header bug | `tasks/15-builtin-rt/20-stdlib-exit-status-macros.md` |
| LUA-003 | `build/rcc/lua -e 'print(42)'` | interpreter links but reports `(command line):1: unexpected symbol`; empty chunks can segfault | runtime codegen/layout bug | `tasks/09-codegen-llvm/31-lua-parser-runtime-regression.md` |

## Exit criteria

- [x] Official source fetched and checksum verified
- [x] Host baseline built
- [x] Host baseline run recorded
- [x] `rcc` build reaches linked interpreter
- [ ] `rcc` runtime output matches host baseline
- [ ] Compiler bugs have minimized regressions
- [ ] `RESULTS.md` updated to pass
