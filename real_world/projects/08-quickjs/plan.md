# 08 — QuickJS probe plan

## Snapshot

- Upstream: <https://bellard.org/quickjs/>
- Commit: `d7ae12ae71dfd6ab2997527d295014a8996fa0f9`
- Local source: `real_world/projects/08-quickjs/upstream/`
- Source policy: never modify upstream `.c`, `.h`, generated tables, or
  `Makefile`; all adaptation belongs in wrapper scripts, checked-in plan files,
  or ignored build outputs.

## Why This Project

QuickJS is a compact but demanding C99 JavaScript engine.  Compared with MuJS it
adds:

- designated initializers inside compound literals used by public value macros;
- `sizeof(type)` arithmetic in bit-field width integer constant expressions;
- hosted Linux/POSIX headers including `signal.h`, pthread-related declarations,
  and `<stdatomic.h>` compatibility;
- a multi-translation-unit runtime with enough surface to expose lowering,
  type-checking, and codegen gaps before a full executable smoke.

## Probe Commands

```sh
bash real_world/projects/08-quickjs/scripts/run-object-probe.sh
bash real_world/projects/08-quickjs/scripts/run-full-qjs-smoke.sh
```

The object-only probe covers the core library sources:

```text
quickjs.c
dtoa.c
libregexp.c
libunicode.c
cutils.c
quickjs-libc.c
```

The full smoke builds a host `qjsc` only to generate `repl.c`, then compiles
`qjs.c`, `repl.c`, and the QuickJS runtime objects with `rcc` and links the
result against host libc/libm/libpthread/libdl.

Both scripts build each source from the same upstream source tree and core
Makefile flags:

```sh
-std=c11 -O2 -fwrapv -funsigned-char -D_GNU_SOURCE \
  -DCONFIG_VERSION="$(cat upstream/VERSION)" -I upstream
```

The host side uses `-std=gnu99`; the rcc side uses `-std=c11` and enables
only the GNU extensions QuickJS actually uses: attributes, range designators,
labels-as-values, inline asm, statement expressions, and common GNU builtin
libcall declarations.

## Current Result

Status: full `qjs` link and rich runtime smoke pass.

The probe is expected to be tightened monotonically:

1. Core object probe passes for all six translation units. Done.
2. `libquickjs.a` archive probe. Covered by the same rcc object set used by the full smoke.
3. `qjs` link probe against host libc/libm/libpthread/libdl. Done.
4. Basic runtime smoke for `qjs -e 'console.log(1 + 2)'`. Done.
5. Rich JavaScript runtime probe for objects, arrays, functions, loops, JSON,
   and generated file input. Done; tracked as QJS-004 in `RESULTS.md`.

## Known Compiler-Owned Findings

| Finding | Owner | Status |
| --- | --- | --- |
| `libregexp.c` uses `sizeof(uintptr_t) * 8 - BP_TYPE_BITS` as a bit-field width. | `rcc_hir_lower` integer constant expression lowering | fixed in local work before this probe is marked pass |
| `quickjs.h` uses an aggregate compound literal as a subobject initializer in `JS_MKVAL`. | `rcc_hir_lower` initializer flattening | fixed in local work before this probe is marked pass |
| `quickjs.c` uses anonymous struct/union members. | `rcc_parse` struct field grammar | fixed in local work before this probe is marked pass |
| `quickjs.c` includes `<stdatomic.h>` and uses `_Atomic(T)` casts. | hosted Linux resource header surface | fixed in local work as a C99 compatibility shim before this probe is marked pass |
| `quickjs.h` uses explicit identity casts such as `(JSValue)v` when `JSValue` is a record. | `rcc_cfg` cast lowering | fixed by treating same-type explicit casts as no-op |
| `quickjs-libc.c` depends on Linux/POSIX declarations (`fd_set`, `popen`, `realpath`, `environ`, extra signals). | hosted Linux resource header surface | fixed by extending checked-in header shims |
| QuickJS uses GNU builtins (`alloca`, `__builtin_clz*`, `__builtin_ctz*`, `__builtin_frame_address`) that must not be left as unresolved libc symbols. | `rcc_codegen_llvm` GNU builtin lowering | fixed by lowering to LLVM alloca/intrinsics |
| `JSCFunctionListEntry` stores pointer-bearing active members inside a union used by static global arrays. | `rcc_codegen_llvm` global initializer materialization | fixed by preserving pointer relocation chunks in union storage |
| `extern const uint8_t qjsc_repl[]` must be representable as an external incomplete array and usable as a pointer. | `rcc_codegen_llvm` global declaration/codegen | fixed by zero-length external declarations plus element alignment |
| `JSClosureVar` starts with bit-fields followed by non-bit-field members at offset 2; LLVM explicit struct stride and bit-field access width must match HIR `sizeof`. | `rcc_codegen_llvm` explicit bit-field record layout | fixed by capping represented bit-field storage at the next non-bit-field offset |

## Follow-up Rule

If this probe fails, do not edit QuickJS source or weaken the selected source
set.  Classify the failure as a specific compiler/header/linker task, fix
`rcc`, then rerun this script.
