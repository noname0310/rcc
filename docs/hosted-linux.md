# Hosted Linux Mode

`rcc` is a C99-first compiler.  Hosted Linux support is an explicit
compatibility policy selected with:

```sh
rcc --linux-gnu-hosted --std=c99 input.c
```

The flag means "compile for a Linux glibc/POSIX hosted environment".  It does
not mean "accept every GNU C extension".  GNU syntax remains behind the existing
feature flags such as `-fgnu-attributes`, `-fgnu-typeof`, `-fgnu-inline-asm`,
and `-fgnu-implicit-function-declaration`.

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
- small declaration shims for headers that are too glibc-internal for the
  current parser surface;
- include ordering between project headers, compiler-provided shims, and host
  headers;
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

`--linux-gnu-hosted` does not enable GNU syntax extensions by itself.  For
example, GNU binary integer literals still require:

```sh
rcc -fgnu-binary-literals input.c
```

Likewise, GNU attributes, statement expressions, `typeof`, inline asm,
computed goto, and implicit function declarations keep their existing explicit
flags and warning policy.

## Relationship To Header Shims

The hosted Linux mode is allowed to add compiler-owned declaration shims under
`lib/rcc/include/`.  These files must stay small and targeted.  They may define
common glibc declaration macros or expose POSIX type/function declarations, but
they must not copy large glibc, musl, or Linux kernel headers wholesale.

Header lookup keeps project code in control:

| Include form | Search order |
| --- | --- |
| `#include "h"` | current file directory, project `-I`, `lib/rcc/include`, `-isystem` / host defaults |
| `#include <h>` | project `-I`, `lib/rcc/include`, `-isystem` / host defaults |

This lets a project deliberately provide its own header through `-I`, while
still allowing selected rcc shims to shadow problematic host system headers
when no project header is present.  Normal host headers remain reachable after
the shim layer.

When a shim declares a function such as `pthread_create`, `dlopen`,
`clock_gettime`, `malloc`, or `printf`, that declaration only lets the frontend
type-check a call.  The implementation is resolved by the host linker and
runtime libraries.

`lib/rcc/include/pthread.h` is a declaration shim for hosted Linux projects. It
declares common pthread entry points such as `pthread_create`, `pthread_join`,
mutex/condition-variable basics, thread-specific storage, and attribute helpers.
The exposed object types (`pthread_attr_t`, `pthread_mutex_t`,
`pthread_cond_t`, and related attr types) are ABI-sized opaque storage
placeholders for the current glibc-oriented probes, not scheduler or locking
implementations. Programs must still link with `-pthread`.

Core POSIX scalar typedefs live in `lib/rcc/include/bits/rcc-posix-types.h` and
are re-exported through `sys/types.h`, `time.h`, `unistd.h`, and `signal.h`.
This keeps `pid_t`, `uid_t`, `gid_t`, `mode_t`, `off_t`, `ssize_t`, `time_t`,
`clockid_t`, and related names single-sourced inside rcc's resource headers.
The current definitions match the LP64 glibc-oriented hosted probes; adding a
layout-sensitive type for another data model requires a target-info-backed
test.

Filesystem-oriented shims live in `fcntl.h`, `dirent.h`, `sys/stat.h`,
`sys/time.h`, and `sys/wait.h`.  `DIR` remains opaque.  `struct dirent` and
`struct stat` expose the fields commonly read by GNU userland probes; the
`struct stat` shape follows the current LP64 glibc field order closely enough
for compile-time field access and simple hosted smoke tests.  Treat it as a
target-specific ABI surface: changes to field order, size, or timestamp storage
need explicit target-layout coverage before they are broadened.

`lib/rcc/include/dlfcn.h` is a declaration shim for hosted Linux dynamic-loader
APIs.  It exposes `dlopen`, `dlsym`, `dlclose`, `dlerror`, `dladdr`, common
`RTLD_*` flags, and `Dl_info`.  The implementation is intentionally external:
the host runtime resolves these symbols during final linking.  Projects that
still require a separate dynamic-loader library must pass `-ldl`; the driver
preserves that explicit flag in the clang/lld-compatible link command and in
missing-linker diagnostics.

## Common Glibc Annotation Macros

`lib/rcc/include/sys/cdefs.h` provides a deliberately small set of glibc
annotation macro shims used by hosted Linux headers:

| Macro family | rcc behavior |
| --- | --- |
| `__BEGIN_DECLS`, `__END_DECLS` | expands to nothing; C++ linkage is not modeled |
| `__THROW`, `__THROWNL`, `__NTH`, `__NTHNL` | strips exception/nothrow annotations while preserving the declarator |
| `__nonnull`, `__wur`, `__attribute_malloc__` | strips function declaration annotations |
| `__attribute_alloc_size__`, `__attr_access`, `__attr_dealloc` | strips allocation/access annotations |
| `__P`, `__PMT` | preserves the prototype argument list |

These definitions are parse/type compatibility only.  They must not grow into a
copy of glibc `sys/cdefs.h`, and they do not provide fortified libc behavior,
symbol redirection, ABI dispatch, or runtime code.

Raw GNU `__attribute__((...))` syntax is also parsed so hosted headers can keep
their declaration shape.  The supported attribute table includes the glibc and
gnulib annotations currently needed by the hosted probes: `nothrow`, `leaf`,
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
- `tasks/16-linux-glibc-compat/04-resource-header-overlay-order.md`
- `tasks/16-linux-glibc-compat/05-glibc-common-macro-shims.md`
- `tasks/16-linux-glibc-compat/08-pthread-driver-flag.md`
- `tasks/16-linux-glibc-compat/16-gnu-coreutils-bootstrap-probe.md`
- `tasks/16-linux-glibc-compat/17-gnu-coreutils-single-utility-probe.md`

Real-world project probes must not weaken source or runtime tests to hide
compiler bugs.  A new hosted Linux failure is either a host-runtime
responsibility, a build-wrapper issue, or a concrete compiler task.
