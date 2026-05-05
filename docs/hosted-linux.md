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

`-pthread` installs `_REENTRANT=1` during preprocessing.  Linker-driver
forwarding for pthread is tracked separately by
`tasks/16-linux-glibc-compat/08-pthread-driver-flag.md`.

## Ownership Boundary

`rcc` owns:

- parsing, type checking, HIR/CFG lowering, and LLVM code generation;
- preprocessor feature-test macro modeling for `_GNU_SOURCE`,
  `_POSIX_C_SOURCE`, `_DEFAULT_SOURCE`, and `_REENTRANT`;
- small declaration shims for headers that are too glibc-internal for the
  current parser surface;
- include ordering between compiler-provided shims and host headers;
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

When a shim declares a function such as `pthread_create`, `dlopen`,
`clock_gettime`, `malloc`, or `printf`, that declaration only lets the frontend
type-check a call.  The implementation is resolved by the host linker and
runtime libraries.

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
