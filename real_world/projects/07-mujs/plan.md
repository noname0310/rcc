# 07 — MuJS probe plan

## Snapshot

- Upstream: <https://mujs.com/>
- Commit: `0b4ed7e4ba37030fdd00f6a17b6de75cd7d7954b`
- Local source: `real_world/projects/07-mujs/upstream/`
- Source policy: never modify upstream `.c`, `.h`, generated Unicode tables, or
  `Makefile`; all adaptation belongs in wrapper scripts or ignored build
  outputs.

## Why This Project

MuJS is a compact hosted C99 JavaScript interpreter.  It is small enough to run
as a smoke test before GNU coreutils, but it still exercises:

- hosted `stdio.h`, `stdlib.h`, `string.h`, `setjmp.h`, and `math.h`
  declarations;
- two-translation-unit linking (`main.c` + `one.c`);
- `-lm` forwarding to the host runtime;
- a real executable with observable output.

## Probe Command

```sh
bash real_world/projects/07-mujs/scripts/run-smoke.sh
```

The script builds a host binary and an `rcc` binary from the same upstream
sources:

```sh
cc -std=c99 -O2 -I upstream upstream/main.c upstream/one.c -lm -o build/mujs-host
rcc --target=x86_64-unknown-linux-gnu --linux-gnu-hosted -std=c99 -O2 \
    -I upstream upstream/main.c upstream/one.c -lm -o build/mujs-rcc
```

It then runs both on:

```js
print(1+2)
```

Expected output:

```text
3
```

## Current Result

Status: pass on WSL/Linux with LLVM 18.

Artifacts are intentionally ignored:

- `build/mujs-host`
- `build/mujs-rcc`
- `build/smoke.js`
- `artifacts/host-mujs-smoke.stdout`
- `artifacts/rcc-mujs-smoke.stdout`
- `logs/*.stdout`, `logs/*.stderr`, and `logs/smoke-output.diff`

## Follow-up Rule

If this probe fails, do not edit MuJS source or weaken the smoke.  Classify the
failure as a specific compiler/header/linker task, fix `rcc`, then rerun this
script.
