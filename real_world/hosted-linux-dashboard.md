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
| Runtime | PASS | Host and rcc outputs both print `3` for `print(1+2)`. | none |

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

## Phase-16 gate

Do not mark `tasks/index.md` phase 16 complete while any dashboard row is
BLOCKED by a compiler-owned task.  At this snapshot the dashboard is current
and phase 16 is complete for the first hosted Linux oracle set.
