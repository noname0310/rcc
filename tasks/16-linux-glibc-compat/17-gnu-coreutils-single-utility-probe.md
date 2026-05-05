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

- [ ] `rcc` compiles the selected utility far enough that all remaining failures
      are linked tasks, or the utility links and runs.
- [ ] If it runs, output/exit status is compared against the host-built utility.
- [ ] The probe is repeatable from a checked-in wrapper script.
- [ ] No failure is dismissed as "coverage percentage"; compiler bugs are fixed.
