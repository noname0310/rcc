# 02 — cJSON results

## Stage 1: `cJSON.c + generated roundtrip.c`

Date: 2026-05-05

Upstream commit: `fb16e5cf358798aabb049655975cde8427101056`

Command:

```sh
real_world/projects/02-cjson/scripts/run-roundtrip.sh
```

Result:

- host compile: pass
- host run: pass
- `rcc` compile/link: pass
- `rcc` run: pass
- stdout comparison: pass

Observed stdout:

```text
{"name":"rcc","answer":42}
```

Compiler follow-up covered by the hosted core declaration sweep:

- `tasks/15-builtin-rt/12-hosted-core-declaration-sweep.md`

Notes:

- Upstream `.c` and `.h` files were not modified.
- Runtime comparison uses a generated smoke program owned by this repository.
- This stage covers one parse/access/print/delete path only. Upstream Unity test
  suites remain future stages.
