# 01 — inih results

## Stage 1: `ini.c + tests/unittest.c`

Date: 2026-05-05

Upstream commit: `577ae2dee1f0d9c2d11c7f10375c1715f3d6940c`

Command:

```sh
real_world/projects/01-inih/scripts/run-unittest-multi.sh
```

Result:

- host compile: pass
- host run: pass
- host stdout diff against `tests/baseline_multi.txt`: pass
- `rcc` compile/link: pass
- `rcc` run: pass
- `rcc` stdout diff against `tests/baseline_multi.txt`: pass

Compiler follow-ups completed during this stage:

- `tasks/15-builtin-rt/09-linux-multiarch-include-discovery.md`
- `tasks/15-builtin-rt/10-ctype-hosted-declarations.md`

Notes:

- Upstream `.c` and `.h` files were not modified.
- Runtime comparison uses upstream's checked-in baseline file.
- This stage covers the default `multi` configuration only. Other upstream test
  variants from `tests/meson.build` remain future stages.

