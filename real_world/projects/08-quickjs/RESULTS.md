# QuickJS Results

Last verified: 2026-05-06 on WSL/Linux with LLVM 18.

## Object Probe

Command:

```sh
LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 \
  RCC_BUILD=0 \
  bash real_world/projects/08-quickjs/scripts/run-object-probe.sh
```

Result: pass.

- Host object build: success
- `rcc` object build: success
- Translation units covered:

```text
quickjs.c
dtoa.c
libregexp.c
libunicode.c
cutils.c
quickjs-libc.c
```

Final output:

```text
quickjs object probe ok (6 translation units)
```

## Full `qjs` Smoke

Command:

```sh
LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 \
  bash real_world/projects/08-quickjs/scripts/run-full-qjs-smoke.sh
```

Result: pass.

- Host `qjsc` build: success
- Host `qjsc` generated `repl.c`: success
- `rcc` build of `qjs.c`, `repl.c`, and QuickJS runtime objects: success
- Link against host libc/libm/libpthread/libdl: success
- Runtime command:

```sh
real_world/projects/08-quickjs/build/full/rcc/qjs -e 'console.log(1 + 2)'
```

- Runtime stdout:

```text
3
```

- Runtime file command:

```sh
real_world/projects/08-quickjs/build/full/rcc/qjs \
  real_world/projects/08-quickjs/build/full/smoke.js
```

- Runtime file stdout:

```text
file:55:42:6:1+2+3
```

Final output:

```text
quickjs full qjs smoke ok
```

## Compiler Bugs Found

| ID | Status | Symptom |
| --- | --- | --- |
| QJS-001 | fixed in current worktree | GNU builtins (`alloca`, `__builtin_clz*`, `__builtin_ctz*`, `__builtin_frame_address`) were left as unresolved symbols instead of lowering to LLVM alloca/intrinsics. |
| QJS-002 | fixed in current worktree | Pointer-bearing active members inside static union initializers lost relocation values and materialized as null. |
| QJS-003 | fixed in current worktree | `extern const uint8_t qjsc_repl[]` needed a zero-length external LLVM declaration with element alignment and pointer use support. |
| QJS-004 | fixed in current worktree | `JSClosureVar` arrays used HIR `sizeof=8` but LLVM explicit bit-field struct stride 10, so QuickJS copied 8-byte entries and then indexed them at 10-byte intervals. The explicit record builder and bit-field access path now cap bit-field storage chunks at the next non-bit-field offset, and the full smoke executes object, array, function, loop, JSON, and file-input probes. |

## Upstream Source Policy

The wrapper does not modify upstream `.c`, `.h`, generated tables, or
`Makefile`. The local `upstream/` tree and build/log outputs are ignored by git.
