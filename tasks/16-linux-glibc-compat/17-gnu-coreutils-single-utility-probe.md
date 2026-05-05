> ✓ done — 2026-05-06

# 16-17: GNU Coreutils Single Utility Probe

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-16-gnu-coreutils-bootstrap-probe  
**Milestone:** hosted-linux

## Goal

Compile one small GNU coreutils utility translation unit with rcc, starting with
`src/true.c`, and convert every real compiler failure into a fix or a task.

## Scope

- In: exact `rcc` command, include order, feature macros, link flags, logs, and
  host comparison command.
- In: fixing compiler bugs discovered by this probe before counting success.
- Out: editing coreutils source or generated headers.

## Acceptance

- [x] `rcc` compiles the selected utility far enough that all remaining failures
      are linked tasks, or the utility links and runs.
- [x] If it runs, output/exit status is compared against the host-built utility.
- [x] The probe is repeatable from a checked-in wrapper script.
- [x] No failure is dismissed as "coverage percentage"; compiler bugs are fixed.

## Result

The checked-in entrypoint is:

```sh
bash real_world/projects/09-gnu-coreutils/scripts/run-true-probe.sh
```

The script reuses the ignored bootstrap/configure worktree from task 16-16,
builds the small generated headers required by `src/true.c`, and asks `rcc` to
lower `src/true.c` with the documented include order. It writes logs under
`real_world/projects/09-gnu-coreutils/logs/true-probe/`.

The selected utility does not yet link/run. The current first compiler-owned
blockers are linked follow-up tasks:

- `tasks/16-linux-glibc-compat/21-gnu-include-next-directive.md` for GNU
  `#include_next` in generated gnulib replacement headers.
- `tasks/16-linux-glibc-compat/22-gnulib-funcdecl-macro-surface.md` for
  `_GL_FUNCDECL_*` / `_GL_CXXALIAS_*` declaration macro cascades.
- `tasks/16-linux-glibc-compat/23-coreutils-posix-declaration-sweep.md` for
  concrete hosted declarations/macros surfaced after replacement headers are
  parsed.
- `tasks/16-linux-glibc-compat/24-coreutils-true-runtime-oracle.md` for the
  final host-vs-rcc runtime comparison.

The host oracle path is still blocked by the task 16-16 generated-header input
issue recorded in `logs/gnulib-config-probe/make-true.stderr`, so this task
does not claim runtime equivalence.
