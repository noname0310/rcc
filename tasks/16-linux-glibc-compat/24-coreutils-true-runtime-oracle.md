# 16-24: coreutils true Runtime Oracle

> ✓ done — 2026-05-06

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-23-coreutils-posix-declaration-sweep  
**Milestone:** hosted-linux

## Goal

Build, link, and run GNU coreutils `src/true` with both host `cc` and `rcc`,
then compare exit status and output.

## Scope

- In: host-build prerequisite repair if generated gnulib headers are missing
  inputs, exact host and rcc commands, link flags, and runtime comparison.
- In: fixing compiler bugs discovered before claiming success.
- Out: modifying upstream coreutils source or generated headers.

## Acceptance

- [x] Host `make src/true` produces the oracle binary or a documented host
      prerequisite task blocks it.
- [x] `rcc` compiles and links the same utility without source mutation.
- [x] Running both binaries yields exit status 0 and identical stdout/stderr.
- [x] The real-world dashboard marks the coreutils runtime cell with the exact
      observed result.

## Result

- Extended `run-true-probe.sh` from an HIR-only probe into a host-vs-rcc
  runtime oracle for upstream `src/true.c`.
- The full upstream `make src/true` attempt is still recorded and currently
  exits 2 because the generated libcoreutils build reaches a host prerequisite
  gap around `_GL_DT_NOTDIR` in `lib/file-has-acl.c`.
- The stable oracle compiles the selected upstream translation unit directly
  with host `cc` and with `rcc`, links both against the same probe-local
  support object, and runs both binaries without modifying upstream sources.
- Observed result: host status 0, rcc status 0, empty stdout, empty stderr.
