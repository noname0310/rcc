# Hosted Linux real-world dashboard

This dashboard is stage-based on purpose.  It does not report a project as a
percentage: a single red compiler-owned stage blocks the project until the
owning task is fixed.

Status legend:

| Status | Meaning |
| --- | --- |
| PASS | Observed locally from the checked-in command. |
| BLOCKED | Not green; the row names the concrete prerequisite or compiler task. |
| TODO | Not attempted yet for this project stage. |

## Summary

| Project | Header/config | Syntax/HIR | Object | Link | Runtime | Current blocker |
| --- | --- | --- | --- | --- | --- | --- |
| MuJS | PASS | PASS | PASS | PASS | PASS | none; smoke output matches host |
| GNU coreutils `src/true` | PASS | PASS | PASS | PASS | PASS | none; direct TU oracle exits 0 with empty stdout/stderr for host and rcc |
| SQLite amalgamation | PASS | PASS | PASS | PASS | PASS | none; checked-in wrapper downloads the official amalgamation into project-local `upstream/` |
| Toybox applet smoke | PASS | BLOCKED | TODO | TODO | TODO | `tasks/16-linux-glibc-compat/25-toybox-applet-hosted-surface.md` |

## MuJS

Command:

```sh
LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 \
  bash real_world/projects/07-mujs/scripts/run-smoke.sh
```

| Stage | Status | Evidence | Next task |
| --- | --- | --- | --- |
| Source acquisition | PASS | `real_world/projects/07-mujs/PROJECT.md` records the upstream source and wrapper policy. | none |
| Header/config | PASS | The probe uses upstream headers plus rcc hosted stdio/stdlib/math declarations. | none |
| Syntax/HIR | PASS | `rcc` compiles `main.c` and `one.c` under `--linux-gnu-hosted`. | none |
| Object | PASS | LLVM backend emits objects for both translation units. | none |
| Link | PASS | `-lm` is forwarded to the host linker driver. | none |
| Runtime | PASS | Host and rcc outputs match for loops, closures, objects, arrays, JSON, regexp, strings, and math; final line is `mujs smoke ok`. | none |

Runtime ownership: MuJS function bodies are compiled from upstream sources by
rcc; libc/libm behavior is supplied by the host.

## GNU coreutils

Bootstrap/config command:

```sh
bash real_world/projects/09-gnu-coreutils/scripts/prepare-local-bootstrap-tools.sh
bash real_world/projects/09-gnu-coreutils/scripts/run-gnulib-config-probe.sh
```

Single utility probe command:

```sh
LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 \
  bash real_world/projects/09-gnu-coreutils/scripts/run-true-probe.sh
```

| Stage | Status | Evidence | Next task |
| --- | --- | --- | --- |
| Source acquisition | PASS | Ignored LF-normalized worktree is created under `build/gnulib-config-probe/src`. | none |
| Bootstrap/configure | PASS | `build/gnulib-config-probe/build/lib/config.h` was generated locally. | none |
| Generated headers | PASS | `run-true-probe.sh` invokes make targets for `lib/configmake.h`, generated replacement headers, and `src/version.h`. | none |
| Syntax/HIR | PASS | `run-true-probe.sh` writes `build/gnulib-config-probe/true.hir`; E0071/E0083 declaration gaps are gone from `logs/true-probe/rcc-true.stderr`. | none |
| Object | PASS | `run-true-probe.sh` writes `true-host.o` with host `cc` and `true-rcc.o` with `rcc --emit=obj`. | none |
| Link | PASS | Both objects link against the same probe-local `true-oracle-support.o`; upstream sources are not modified. | none |
| Runtime | PASS | `host-run.status` and `rcc-run.status` are both 0; stdout/stderr logs are empty and byte-identical. | none |

The current compiler-owned queue is empty for the first `src/true.c` runtime
oracle.  The full upstream `make src/true` path is still logged separately and
currently exits 2 on a generated libcoreutils prerequisite gap around
`_GL_DT_NOTDIR` in `lib/file-has-acl.c`; that is not used as the stable
single-TU compiler oracle.

Runtime ownership: GNU coreutils runtime behavior comes from upstream sources
plus host glibc/libpthread/libdl/libm.  rcc owns the compile pipeline and link
flag orchestration, not replacement libc bodies.

## SQLite amalgamation

CLI probe command sequence is recorded in
`projects/06-sqlite-amalgamation/PROJECT.md`. The wrapper downloads the
official amalgamation into this project's ignored `upstream/` directory,
compiles `sqlite3.c` and `shell.c` separately with
`rcc --linux-gnu-hosted --std=c11`, links them with host `cc -ldl -lm`, and runs
an in-memory SQL smoke.

| Stage | Status | Evidence | Next task |
| --- | --- | --- | --- |
| Source acquisition | PASS | Wrapper uses official amalgamation files under `real_world/projects/06-sqlite-amalgamation/upstream/sqlite-amalgamation-3530000/`, downloading the zip on demand. | none |
| Header/config | PASS | Hosted Linux mode plus SQLite feature macros cover the recorded CLI smoke; `SQLITE_OMIT_VIRTUALTABLE` is intentionally not used. | none |
| Syntax/HIR | PASS | `sqlite3.c` and `shell.c` compile through preprocessing, parsing, HIR lowering, and typeck. | none |
| Object | PASS | `sqlite3.rcc.o` and `shell.rcc.o` are emitted by the LLVM backend. | none |
| Link | PASS | Host `cc` links the two rcc objects with `-ldl -lm`. | none |
| Runtime | PASS | `CREATE TABLE t(x); INSERT INTO t VALUES(1); SELECT * FROM t;` on `:memory:` prints `1` without a CLI crash. | none |

Runtime ownership: SQLite code is compiled from the amalgamation by rcc; libc,
libm, libdl, and process startup remain host responsibilities.

## Toybox

Applet smoke command:

```sh
LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 \
  bash real_world/projects/10-toybox/scripts/run-applet-smoke.sh
```

| Stage | Status | Evidence | Next task |
| --- | --- | --- | --- |
| Source acquisition | PASS | Ignored LF-normalized worktree is created under `real_world/projects/10-toybox/upstream`. | none |
| Host baseline | PASS | `scripts/single.sh true false echo cat wc` builds with host `cc` and the wrapper records per-applet run logs. | none |
| Syntax/HIR | BLOCKED | The `rcc` compile path stops while compiling the first applet source set. C11 `_Noreturn` and `sigjmp_buf` no longer appear; current diagnostics include `timer_t`, `SIGKILL`, `SIGWINCH`, `stpcpy`, `syscall`, `netinet/tcp.h`, and duplicate `timespec`/`timeval` gaps. | `16-25` |
| Object | TODO | Not reached. | `16-25` |
| Link | TODO | Not reached. | `16-25` |
| Runtime | TODO | Not reached. | `16-25` |

Runtime ownership: Toybox runtime behavior should come from upstream sources
plus host glibc. rcc owns the compile pipeline, hosted header model, and link
flag orchestration, not replacement libc bodies.

## Phase-16 gate

Do not mark `tasks/index.md` phase 16 complete while any dashboard row is
BLOCKED by a compiler-owned task. At this snapshot Toybox has reopened phase 16
with task `16-25`.
