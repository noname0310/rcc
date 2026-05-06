# Hosted Linux Mode

`rcc` supports C99 and C11 language modes. Hosted Linux support is an explicit
compatibility policy selected with:

```sh
rcc --linux-gnu-hosted --std=c11 input.c
```

The flag means "compile for a Linux glibc/POSIX hosted environment".  It does
not mean "accept every GNU C extension".  GNU syntax remains behind the existing
feature flags such as `-fgnu-attributes`, `-fgnu-typeof`, `-fgnu-inline-asm`,
and `-fgnu-implicit-function-declaration`.  Hosted mode also does not choose a
language standard: use `-std=c99` or `-std=c11` explicitly when the source needs
a particular ISO mode.

In this mode the driver installs feature-test macros through the same path as
ordinary command-line `-D` definitions:

| Macro | Default value |
| --- | --- |
| `_GNU_SOURCE` | `1` |
| `_DEFAULT_SOURCE` | `1` |
| `_POSIX_C_SOURCE` | `200809L` |
| `_XOPEN_SOURCE` | `700` |

User `-D` definitions are appended after these defaults, so a project wrapper
can intentionally choose a narrower feature level.  User `-U` flags still run
after all definitions and may remove any of the defaults.

`-pthread` installs `_REENTRANT=1` during preprocessing and forwards
`-pthread` to the clang-compatible linker driver during final linking.  The
flag is rejected for Windows targets instead of being silently ignored.

## Ownership Boundary

`rcc` owns:

- parsing, type checking, HIR/CFG lowering, and LLVM code generation;
- preprocessor feature-test macro modeling for `_GNU_SOURCE`,
  `_POSIX_C_SOURCE`, `_DEFAULT_SOURCE`, and `_REENTRANT`;
- compiler-owned headers that describe frontend builtins or language support,
  such as `stddef.h`, `stdarg.h`, `stdint.h`, and `stdatomic.h`;
- include ordering between project headers, compiler-owned headers, and host
  sysroot headers;
- forwarding hosted link flags such as `-lm`, `-pthread`, and `-ldl` to the
  selected clang-compatible linker driver.

The host platform owns:

- libc/libm/libpthread/libdl function bodies;
- CRT startup files, dynamic loader behavior, errno storage, TLS, syscalls, and
  kernel ABI details;
- the final native linker implementation.  `rcc` drives a clang/lld-compatible
  external toolchain rather than shipping a native linker.

## Strictness

Strict C99 remains the default:

```sh
rcc --std=c99 input.c
```

`-std=c11` enables C11 language/preprocessor semantics as they are implemented,
including `__STDC_VERSION__ == 201112L`.  `--linux-gnu-hosted` does not enable
GNU syntax extensions or change the selected ISO standard by itself.  For
example, GNU binary integer literals still require:

```sh
rcc -fgnu-binary-literals input.c
```

Likewise, GNU attributes, statement expressions, `typeof`, inline asm,
computed goto, and implicit function declarations keep their existing explicit
flags and warning policy.

## Relationship To Header Shims

Hosted Linux mode must not add approximate libc, POSIX, glibc, musl, or Linux
kernel header replacements under `lib/rcc/include/`. Those ABI-visible headers
belong to the selected host sysroot. When a real project exposes a host-header
failure, the preferred fix is to teach the frontend to preprocess, parse,
lower, or type-check the real header form.

The only checked-in headers under `lib/rcc/include/` are compiler-owned
resource headers: language macros, builtin typedefs, target scalar limits, and
frontend surfaces such as `__builtin_va_list` or C11 atomics that require direct
compiler cooperation. They are not a libc overlay.

Header lookup keeps project code in control:

| Include form | Search order |
| --- | --- |
| `#include "h"` | current file directory, project `-I`, `lib/rcc/include`, `-isystem` / host defaults |
| `#include <h>` | project `-I`, `lib/rcc/include`, `-isystem` / host defaults |

This lets a project deliberately provide its own header through `-I`, while
keeping compiler-owned headers deterministic. Because rcc no longer ships libc
or POSIX declaration shims, ordinary hosted headers such as `stdio.h`,
`pthread.h`, `unistd.h`, `sys/stat.h`, or `netinet/tcp.h` resolve from the host
sysroot.

## Compiler-Owned Header Inventory

The resource directory is intentionally small:

| Header | Owner | Notes |
| --- | --- | --- |
| `stddef.h` | compiler | `size_t`, `ptrdiff_t`, `wchar_t`, `NULL`, and `offsetof` definitions tied to rcc target layout/lowering |
| `stdarg.h` | compiler | `va_list` and builtin varargs hooks |
| `stdint.h`, `limits.h`, `float.h` | compiler | target scalar limits and integer typedefs |
| `stdbool.h`, `iso646.h`, `stdalign.h`, `stdnoreturn.h` | compiler | language macro surfaces |
| `stdatomic.h` | compiler | C11 atomic macro/type surface that maps onto rcc atomic lowering |

Do not add `stdio.h`, `stdlib.h`, `string.h`, `pthread.h`, `unistd.h`,
`sys/*.h`, or Linux networking headers here. If one of those headers fails, add
a minimized compiler bug task and keep using the host header as the oracle.

## C11 Library Surface

C11 coverage is intentionally split into compiler-owned language support and
host-owned library headers/runtime bodies:

| Header | Status | Runtime owner / deferred work |
| --- | --- | --- |
| `stdalign.h` | implemented macro surface: `alignas`, `alignof`, `__alignas_is_defined`, `__alignof_is_defined` | no runtime |
| `stdnoreturn.h` | implemented macro surface: `noreturn`, `__noreturn_is_defined` | no runtime |
| `stdatomic.h` | declaration/macro surface for atomic typedefs, lock-free macros, memory-order constants, simple load/store/fetch helpers, `atomic_flag`, and fences | rcc lowers atomic lvalue load/store to LLVM atomics; full generic-operation lowering and link-free `atomic_flag_*` bodies are deferred compiler/runtime work |
| `float.h` | C11 decimal-digit/subnormal macro deltas for the current LP64/Linux and Windows baselines | target-info-backed generation is deferred |

Hosted C11 library headers such as `assert.h`, `threads.h`, `uchar.h`,
`stdlib.h`, and `time.h` are part of the C11 target, but they are supplied by
the real target sysroot in hosted mode. Do not paper over host-header failures
with copied libc headers; add a compiler task when a real source file reaches a
host-header parse or lowering bug.

Annex K bounds-checking interfaces, the analyzability annex, and a C11 threads
runtime independent of the host libc remain optional/deferred unless a
conformance or real-world probe requires them.

The repeatable hosted header gate is:

```sh
cargo test -p rcc_driver --test hosted_linux_headers
```

It runs only on Linux hosts and lowers representative fixtures through
`--emit=hir`: core hosted C99 declarations used by inih/cJSON/Lua/MuJS,
filesystem/POSIX declarations used by GNU coreutils, and pthread/dlfcn
declarations used by QuickJS and dynamic-loading probes.  Failures must become
specific compiler or header-surface tasks; the gate does not use broad xfails.

The pthread runtime smoke fixture is
`crates/rcc_driver/tests/fixtures/pthread_runtime_smoke.c`.  On Linux with the
LLVM backend enabled, this gate compiles, links, and runs it through host
pthread:

```sh
LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 RCC_RUN_LINK_E2E=1 \
  cargo test -p rcc_driver --features rcc_codegen_llvm/llvm \
  --test linker_flags e2e_link_with_pthread_when_enabled -- --nocapture
```

Non-Linux targets must reject `-pthread` clearly instead of silently compiling a
program that cannot link against a pthread implementation.

## Common Glibc Header Forms

Raw GNU `__attribute__((...))` syntax is parsed so hosted headers can keep their
declaration shape.  The supported attribute table includes the glibc and gnulib
annotations currently needed by the hosted probes: `nothrow`, `leaf`,
`nonnull`, `pure`, `const`, `malloc`, `format`, `warn_unused_result`,
`visibility`, `deprecated`, `aligned`, `packed`, `section`, `weak`,
`alloc_size`, `alloc_align`, `access`, `copy`, and related spelling variants
with leading/trailing double underscores.  Unknown attributes are recovered with
W0033 instead of being silently treated as semantically supported.

Hosted Linux mode also normalizes GNU qualifier aliases used by glibc headers:
`__restrict`, `__restrict__`, and `__restrict_arr` map to C99 `restrict`;
`__const` / `__const__` map to `const`; and `__volatile` / `__volatile__` map
to `volatile`.  These spellings are not accepted by strict C99 mode unless the
explicit `-fgnu-qualifier-aliases` option is used.  Lowering records pointer
parameter qualifiers in `ObjectQuals`; for array parameters, qualifiers inside
`[...]` qualify the adjusted pointer parameter, not the array element.

## Current Probe Queue

The phase-16 task tree is the authoritative hosted Linux queue:

- `tasks/16-linux-glibc-compat/03-feature-test-macro-model.md`
- `tasks/16-linux-glibc-compat/08-pthread-driver-flag.md`
- `tasks/16-linux-glibc-compat/14-glibc-system-header-parse-gate.md`
- `tasks/16-linux-glibc-compat/16-gnu-coreutils-bootstrap-probe.md`
- `tasks/16-linux-glibc-compat/17-gnu-coreutils-single-utility-probe.md`
- `tasks/16-linux-glibc-compat/21-gnu-include-next-directive.md`
- `tasks/16-linux-glibc-compat/22-gnulib-funcdecl-macro-surface.md`
- `tasks/16-linux-glibc-compat/23-coreutils-posix-declaration-sweep.md`
- `tasks/16-linux-glibc-compat/24-coreutils-true-runtime-oracle.md`

Real-world project probes must not weaken source or runtime tests to hide
compiler bugs.  A new hosted Linux failure is either a host-runtime
responsibility, a build-wrapper issue, or a concrete compiler task.
