# 16-linux-glibc-compat

**Goal of the phase.** Make `rcc` usable as a Linux hosted C compiler for
real projects whose source includes glibc, POSIX, and GNU/Linux headers.

This phase does **not** implement glibc. Runtime code remains owned by the host
platform: glibc, libm, libpthread, libdl, librt where applicable, and the
clang/lld-compatible linker driver. The compiler-owned work is parsing,
type-checking, lowering, and linking against that hosted ABI.

## Policy

- Do not vendor glibc, musl, or Linux kernel headers.
- Do not copy large system headers into `lib/rcc/include`.
- Do add small declaration shims when a real project repeatedly exposes a
  header surface that is too GNU/glibc-internal for the current parser.
- Do add compiler support for syntax and type-system constructs needed to parse
  hosted headers: feature-test macros, GNU attributes, `_Atomic`, `__restrict`,
  inline asm forms, anonymous records, and constant-expression cases.
- Do keep function bodies external. Calls such as `pthread_create`, `dlopen`,
  `clock_gettime`, `malloc`, and `printf` resolve at link time.

## Tasks

| # | File | Summary |
|---|------|---------|
| 01 | [`01-hosted-linux-surface-audit.md`](01-hosted-linux-surface-audit.md) | Inventory glibc/POSIX blockers from MuJS, GNU coreutils, SQLite, and future probes. |
| 02 | [`02-compat-mode-and-policy.md`](02-compat-mode-and-policy.md) | Define the `linux-gnu-hosted` compatibility mode and strictness boundaries. |
| 03 | [`03-feature-test-macro-model.md`](03-feature-test-macro-model.md) | Model `_GNU_SOURCE`, `_POSIX_C_SOURCE`, `_DEFAULT_SOURCE`, `_REENTRANT`. |
| 04 | [`04-resource-header-overlay-order.md`](04-resource-header-overlay-order.md) | Make rcc shims shadow only selected high-risk headers before host headers. |
| 05 | [`05-glibc-common-macro-shims.md`](05-glibc-common-macro-shims.md) | Provide `__THROW`, `__nonnull`, `__wur`, decl begin/end and related macros. |
| 06 | [`06-gnu-header-attribute-tolerance.md`](06-gnu-header-attribute-tolerance.md) | Accept no-op/semantic GNU attributes used by glibc headers. |
| 07 | [`07-restrict-and-qualifier-aliases.md`](07-restrict-and-qualifier-aliases.md) | Normalize `__restrict`, `__restrict_arr`, and glibc qualifier spellings. |
| 08 | [`08-pthread-driver-flag.md`](08-pthread-driver-flag.md) | Implement `-pthread` as compile define plus linker driver flag. |
| 09 | [`09-pthread-header-shim.md`](09-pthread-header-shim.md) | Add minimal hosted `pthread.h` declarations and type surface. |
| 10 | [`10-posix-core-type-shims.md`](10-posix-core-type-shims.md) | Cover `sys/types.h`, `unistd.h`, `time.h`, `signal.h` core POSIX types. |
| 11 | [`11-fcntl-dirent-stat-shims.md`](11-fcntl-dirent-stat-shims.md) | Cover file-system oriented POSIX surfaces used by GNU userland. |
| 12 | [`12-dlfcn-and-runtime-linking.md`](12-dlfcn-and-runtime-linking.md) | Support `<dlfcn.h>` declarations and runtime library linkage. |
| 13 | [`13-gnulib-config-header-probe.md`](13-gnulib-config-header-probe.md) | Parse generated gnulib `config.h` and selected replacement headers. |
| 14 | [`14-glibc-system-header-parse-gate.md`](14-glibc-system-header-parse-gate.md) | Add parse/typecheck smoke tests for representative glibc headers. |
| 15 | [`15-mujs-hosted-smoke.md`](15-mujs-hosted-smoke.md) | Record and automate the MuJS compile+run smoke. |
| 16 | [`16-gnu-coreutils-bootstrap-probe.md`](16-gnu-coreutils-bootstrap-probe.md) | Bootstrap/configure GNU coreutils with the host toolchain and record generated surfaces. |
| 17 | [`17-gnu-coreutils-single-utility-probe.md`](17-gnu-coreutils-single-utility-probe.md) | Compile one small GNU coreutils utility with `rcc` and turn blockers into compiler fixes. |
| 18 | [`18-posix-thread-runtime-smoke.md`](18-posix-thread-runtime-smoke.md) | Add a minimal pthread compile+link+run regression. |
| 19 | [`19-header-shim-audit-docs.md`](19-header-shim-audit-docs.md) | Document what rcc owns vs host glibc owns. |
| 20 | [`20-real-world-glibc-dashboard.md`](20-real-world-glibc-dashboard.md) | Add a dashboard row for hosted Linux real-world projects. |

## Exit Criteria

- A small pthread program compiles, links, and runs with `rcc -pthread`.
- MuJS compiles and runs the tiny JavaScript smoke under WSL/Linux.
- GNU coreutils has a reproducible host-bootstrap record, and at least one small
  utility is compiled by `rcc` far enough that every remaining failure is an
  explicit compiler task rather than an untriaged glibc-header parse failure.
- `rcc` never pretends to implement glibc bodies; all runtime symbols are linked
  through host libraries.
