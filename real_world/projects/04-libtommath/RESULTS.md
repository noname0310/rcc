# LibTomMath results

## 2026-05-05 smoke

**Snapshot:** `ae40a87a920099a7d9d00979570e0c8d917a1fd7`

**Command:**

```sh
bash real_world/projects/04-libtommath/scripts/run-smoke.sh
```

**Result:** pass.

- Host baseline: `gcc -std=c99 -Wall -Wextra`
- Host stdout: `12193263112482853211126352690`
- `rcc` command: `target/release/rcc -j 8 --std=c99 -Wall`
- `rcc` stdout: `12193263112482853211126352690`
- Runtime oracle: exact stdout comparison with the host compiler baseline

The wrapper compiles every top-level LibTomMath library translation unit:

```text
mp_*.c s_*.c ../scratch/libtommath_smoke.c
```

## Compiler bugs found

| ID | Fixed by | Symptom |
| --- | --- | --- |
| LTM-001 | `tasks/15-builtin-rt/19-posix-errno-constants.md` | builtin `errno.h` lacked POSIX constants required by `s_mp_rand_platform.c` |
| LTM-002 | `tasks/10-driver/19-parallel-multi-input-object-builds.md` | 161 translation units made the serial multi-input driver hit the operational timeout |
| LTM-003 | `tasks/08-cfg/29-constant-condition-dead-branch-pruning.md` | `MP_HAS(...)` constant false branches leaked disabled platform calls at `-O0` |

## Upstream source policy

The wrapper does not modify upstream C or header files. The local `upstream/`
clone is ignored by git. The smoke source is generated under ignored
`scratch/` and belongs to the probe, not to upstream LibTomMath.
