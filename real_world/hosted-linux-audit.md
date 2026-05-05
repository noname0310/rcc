# Hosted Linux Surface Audit

This audit tracks the Linux hosted surfaces exposed by real-world probes.  It is
not a libc implementation plan: `rcc` owns parsing, type checking, lowering,
codegen, and linker-driver orchestration, while function bodies and ABI runtime
semantics remain host responsibilities provided by glibc/libm/libpthread/libdl
and the selected linker driver.

## Summary

| Project | Current status | Hosted surface | First compiler-owned blockers |
| --- | --- | --- | --- |
| inih | pass | glibc multiarch includes, `<ctype.h>` | fixed by `tasks/15-builtin-rt/09-linux-multiarch-include-discovery.md`, `tasks/15-builtin-rt/10-ctype-hosted-declarations.md` |
| cJSON | pass | numeric parsing declarations from hosted libc | fixed by `tasks/15-builtin-rt/12-hosted-core-declaration-sweep.md` |
| zlib | pass | mostly freestanding C with hosted link/run smoke | fixed by `tasks/04-preprocess/22-multiline-function-macro-invocation.md`, `tasks/09-codegen-llvm/30-external-incomplete-array-globals.md`, `tasks/08-cfg/28-string-literal-index-place.md`, `tasks/07-typeck/24-casted-string-global-initializer.md` |
| LibTomMath | pass | `errno.h`, multi-input link scalability, platform guards | fixed by `tasks/15-builtin-rt/19-posix-errno-constants.md`, `tasks/10-driver/19-parallel-multi-input-object-builds.md`, `tasks/08-cfg/29-constant-condition-dead-branch-pruning.md` |
| Lua | pass | `<stdlib.h>`, `-lm`, large hosted executable | fixed by `tasks/06-hir-lower/33-array-bound-ice-constants.md`, `tasks/15-builtin-rt/20-stdlib-exit-status-macros.md`, `tasks/09-codegen-llvm/31-lua-parser-runtime-regression.md` |
| SQLite amalgamation | planned | large single translation unit, hosted declarations | no recorded blocker yet; see `real_world/projects/06-sqlite-amalgamation/PROJECT.md` |
| MuJS | pass | math/stdio/stdlib hosted declarations, JavaScript executable | fixed by `tasks/16-linux-glibc-compat/15-mujs-hosted-smoke.md` |
| QuickJS | partial object probe | `<stdatomic.h>`, pthread/glibc headers, anonymous bit-field / ICE cases | tasks `16-06-gnu-header-attribute-tolerance.md`, `16-07-restrict-and-qualifier-aliases.md`, `16-09-pthread-header-shim.md`, `16-10-posix-core-type-shims.md`, plus `14-lang-extensions`/typeck follow-ups as needed |
| GNU coreutils | source cloned; bootstrap not run in tracked scripts yet | gnulib `config.h`, glibc/POSIX/GNU headers, generated replacement headers | tasks `16-03` through `16-17`; first target utility is `src/true.c` |

## Classification Rules

| Class | Owned by | Examples |
| --- | --- | --- |
| Language / parser | `rcc_parse`, `rcc_hir_lower`, `rcc_typeck` | GNU attributes, qualifier aliases, `_Atomic`, anonymous records, integer constant expressions in bit-fields |
| Preprocessor | `rcc_preprocess` | feature-test macros, computed include forms, glibc compatibility macros |
| System-header discovery | `rcc_driver`, `rcc_session`, `rcc_preprocess` | multiarch include directories, overlay ordering, `-isystem` handling |
| Header shim declaration surface | `lib/rcc/include` plus tests | small hosted declarations and macros for high-risk headers |
| Linker orchestration | `rcc_driver` | `-pthread`, `-lm`, `-ldl`, linker driver selection |
| Runtime implementation | host system | function bodies for `printf`, `malloc`, `pthread_create`, `dlopen`, `clock_gettime`, errno storage, syscall behavior |

## Existing Project Findings

### inih

Repro record:

```sh
bash real_world/projects/01-inih/scripts/run-unittest-multi.sh
```

Known blockers from `real_world/projects/01-inih/plan.md`:

| ID | Classification | Status |
| --- | --- | --- |
| INIH-001 | Linux GNU multiarch include discovery | fixed by `tasks/15-builtin-rt/09-linux-multiarch-include-discovery.md` |
| INIH-002 | compiler-owned hosted `<ctype.h>` declaration shim | fixed by `tasks/15-builtin-rt/10-ctype-hosted-declarations.md` |
| INIH-003 | local linker-driver spelling | wrapper uses `clang-18`; no compiler fix required |

Runtime ownership: `isspace` and libc startup/runtime come from host libc.

### cJSON

Repro record:

```sh
bash real_world/projects/02-cjson/scripts/run-roundtrip.sh
```

Known blocker:

| ID | Classification | Status |
| --- | --- | --- |
| CJSON-001 | compiler-owned hosted numeric declaration shim for `strtod` / `sscanf` | fixed by `tasks/15-builtin-rt/12-hosted-core-declaration-sweep.md` |

Runtime ownership: numeric conversion and formatted scanning function bodies are
host libc responsibilities.

### zlib

Repro record:

```sh
bash real_world/projects/03-zlib/scripts/run-smoke.sh
```

Known blockers:

| ID | Classification | Status |
| --- | --- | --- |
| ZLIB-001 | preprocessor multiline function-like macro invocation | fixed by `tasks/04-preprocess/22-multiline-function-macro-invocation.md` |
| ZLIB-002 | LLVM codegen external incomplete array globals | fixed by `tasks/09-codegen-llvm/30-external-incomplete-array-globals.md` |
| ZLIB-003 | CFG string-literal subscript lvalue lowering | fixed by `tasks/08-cfg/28-string-literal-index-place.md` |
| ZLIB-004 | typeck const-eval for casted string literal pointer initializers | fixed by `tasks/07-typeck/24-casted-string-global-initializer.md` |

Runtime ownership: zlib's generated smoke links with host startup/libc; zlib
function bodies are compiled from upstream sources by `rcc`.

### LibTomMath

Repro record:

```sh
bash real_world/projects/04-libtommath/scripts/run-smoke.sh
```

Known blockers:

| ID | Classification | Status |
| --- | --- | --- |
| LTM-001 | compiler-owned `errno.h` hosted constants | fixed by `tasks/15-builtin-rt/19-posix-errno-constants.md` |
| LTM-002 | driver multi-input scalability | fixed by `tasks/10-driver/19-parallel-multi-input-object-builds.md` |
| LTM-003 | CFG constant-condition pruning | fixed by `tasks/08-cfg/29-constant-condition-dead-branch-pruning.md` |

Runtime ownership: errno storage and platform calls remain host libc behavior;
`rcc` owns declarations/macros and dead-code removal that prevents disabled
platform branches from reaching link time.

### Lua

Repro record:

```sh
bash real_world/projects/05-lua/scripts/run-smoke.sh
```

Known blockers:

| ID | Classification | Status |
| --- | --- | --- |
| LUA-001 | HIR lowering of ICE array bounds using enum constants, casts, and `offsetof` | fixed by `tasks/06-hir-lower/33-array-bound-ice-constants.md` |
| LUA-002 | compiler-owned `<stdlib.h>` `EXIT_SUCCESS` / `EXIT_FAILURE` macros | fixed by `tasks/15-builtin-rt/20-stdlib-exit-status-macros.md` |
| LUA-003 | LLVM record layout for structs containing unions | fixed by `tasks/09-codegen-llvm/31-lua-parser-runtime-regression.md` |

Runtime ownership: `-lm` and libc function bodies are host libraries.  `rcc`
must pass them through to the linker driver; it must not implement libm.

### SQLite

Current record: `real_world/projects/06-sqlite-amalgamation/PROJECT.md`.
No compile log has been recorded yet.  Expected hosted surfaces are large
single-TU preprocessing, libc declarations, file APIs, and optional threading
macros.  New failures must become concrete compiler tasks rather than probe
workarounds.

### MuJS

Current record: `real_world/projects/07-mujs/plan.md`; the reproducible command
is:

```sh
bash real_world/projects/07-mujs/scripts/run-smoke.sh
```

The probe builds `main.c` + `one.c` with both host `cc` and `rcc`, links with
`-lm`, runs `print(1+2)`, and compares output.

Expected surfaces:

- hosted stdio/stdlib/math declarations;
- `-lm` linker propagation;
- executable smoke comparison against a host build.

Runtime ownership: JavaScript runtime function bodies are MuJS sources compiled
by `rcc`; libc/libm symbols are host runtime symbols.

### QuickJS

Current record: `real_world/projects/08-quickjs/PROJECT.md` plus ignored object
probe logs under `real_world/projects/08-quickjs/build/rcc/`.

Observed blockers:

| Symptom | Classification | Owning task |
| --- | --- | --- |
| `cannot find header stdatomic.h` from `quickjs.c` / `quickjs-libc.c` | hosted header surface / `_Atomic` compatibility | `tasks/16-linux-glibc-compat/10-posix-core-type-shims.md` and follow-up typeck tasks if `_Atomic` semantics are needed |
| parse failures in `/usr/include/pthread.h` around `__clockid_t`, `__abstime`, `__nonnull`, `__THROWNL`, `__restrict` | glibc macro/attribute/qualifier tolerance | `tasks/16-linux-glibc-compat/05-glibc-common-macro-shims.md`, `06-gnu-header-attribute-tolerance.md`, `07-restrict-and-qualifier-aliases.md`, `09-pthread-header-shim.md` |
| `libregexp.c` bit-field width using `sizeof(uintptr_t) * 8 - BP_TYPE_BITS` rejected as non-ICE | type checker constant-expression gap | create a focused `07-typeck` task when reduced from the QuickJS probe |
| follow-on `record has no member named val` after the failed bit-field | secondary error | not independently actionable until the bit-field ICE bug is fixed |

Runtime ownership: pthread, atomics implementation details, libc, and libm are
host responsibilities.  `rcc` owns parsing declarations and passing link flags.

### GNU coreutils

Current record: `real_world/projects/09-gnu-coreutils/plan.md`.

First target utility: `src/true.c`.

Planned repro flow:

```sh
cd real_world/projects/09-gnu-coreutils
# Task 16-16 owns the concrete bootstrap/configure command and generated logs.
# Task 16-17 owns the first rcc compile command for src/true.c.
```

Expected first surfaces:

| Surface | Classification | Owning task |
| --- | --- | --- |
| gnulib-generated `config.h` and replacement-header include order | build probe / preprocessor include ordering | `tasks/16-linux-glibc-compat/13-gnulib-config-header-probe.md`, `16-gnu-coreutils-bootstrap-probe.md` |
| `_GNU_SOURCE`, `_POSIX_C_SOURCE`, `_DEFAULT_SOURCE`, `_REENTRANT` | feature-test macro model | `tasks/16-linux-glibc-compat/03-feature-test-macro-model.md` |
| `__THROW`, `__wur`, `__nonnull`, `__attribute_malloc__` | glibc compatibility macros / GNU attributes | `tasks/16-linux-glibc-compat/05-glibc-common-macro-shims.md`, `06-gnu-header-attribute-tolerance.md` |
| `sys/types.h`, `sys/stat.h`, `fcntl.h`, `dirent.h`, `unistd.h` | POSIX declaration shims | `tasks/16-linux-glibc-compat/10-posix-core-type-shims.md`, `11-fcntl-dirent-stat-shims.md` |
| `pthread_*`, `dlopen`, `clock_gettime`, `malloc`, `printf` | host runtime symbols | compile/link flags and declarations only; no rcc function bodies |

Runtime ownership: GNU coreutils runtime behavior is supplied by upstream
sources plus host glibc/libpthread/libdl/libm.  `rcc` must not ship replacement
implementations for these bodies.

## Open Compiler-Owned Work Queue

The phase-16 task order is the working queue for hosted Linux.  If a project
probe discovers a new compiler-owned blocker, add a focused task to the owning
phase rather than weakening the real-world probe.

- Feature-test macro model: `tasks/16-linux-glibc-compat/03-feature-test-macro-model.md`
- Header overlay and glibc macro shims: `tasks/16-linux-glibc-compat/04-resource-header-overlay-order.md`,
  `05-glibc-common-macro-shims.md`
- GNU attributes and qualifier aliases: `tasks/16-linux-glibc-compat/06-gnu-header-attribute-tolerance.md`,
  `07-restrict-and-qualifier-aliases.md`
- Hosted pthread/POSIX/dlfcn surface: `tasks/16-linux-glibc-compat/08-pthread-driver-flag.md`
  through `12-dlfcn-and-runtime-linking.md`
- Generated gnulib/coreutils probes: `tasks/16-linux-glibc-compat/13-gnulib-config-header-probe.md`
  through `17-gnu-coreutils-single-utility-probe.md`

## Audit Invariants

- Do not vendor glibc, musl, or Linux kernel headers wholesale.
- Do not edit upstream project `.c` or `.h` sources to hide an `rcc` bug.
- Do not add xfail-style project skips for compiler bugs.
- Do not implement libc/libm/libpthread/libdl function bodies in `rcc`.
- Do add small declaration shims, parser support, and driver/linker behavior
  when real hosted projects repeatedly expose the same surface.
