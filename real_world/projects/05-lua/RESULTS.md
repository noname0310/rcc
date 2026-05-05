# Lua results

## 2026-05-06 smoke

**Snapshot:** Lua 5.5.0 official tarball

**Command:**

```sh
bash real_world/projects/05-lua/scripts/run-smoke.sh
```

**Result:** pass.

- Host baseline: `gcc -std=c99 -Wall -Wextra`
- Host stdout:

```text
Lua 5.5
42
```

- `rcc` command: `target/release/rcc -j 8 --std=c99 -Wall`
- `rcc` build/link result: success
- `rcc -v` output:

```text
Lua 5.5.0  Copyright (C) 1994-2025 Lua.org, PUC-Rio
```

- `rcc` runtime command:

```sh
build/rcc/lua -e 'print(_VERSION); print(6*7)'
```

- `rcc` runtime stdout:

```text
Lua 5.5
42
```

The previous runtime failure was reduced to an LLVM record layout bug in
`tasks/09-codegen-llvm/31-lua-parser-runtime-regression.md`: unions were lowered
as align-1 byte arrays, so structs containing unions could be too small in LLVM
IR and a large zero-init could clobber adjacent stack slots.

## Compiler bugs found

| ID | Status | Symptom |
| --- | --- | --- |
| LUA-001 | fixed in current worktree | HIR lowering tagged enum/cast/`offsetof` array bounds as VLAs |
| LUA-002 | fixed in current worktree | builtin `<stdlib.h>` missed `EXIT_SUCCESS` / `EXIT_FAILURE` |
| LUA-003 | fixed | union-in-struct LLVM layout clobbered a live stack parameter |

## Upstream source policy

The wrapper does not modify upstream C or header files. The local `upstream/`
tree is ignored by git. Generated smoke files live under ignored `scratch/`,
`logs/`, `artifacts/`, and `build/`.
