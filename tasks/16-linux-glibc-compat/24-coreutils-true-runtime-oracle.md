# 16-24: coreutils true Runtime Oracle

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

- [ ] Host `make src/true` produces the oracle binary or a documented host
      prerequisite task blocks it.
- [ ] `rcc` compiles and links the same utility without source mutation.
- [ ] Running both binaries yields exit status 0 and identical stdout/stderr.
- [ ] The real-world dashboard marks the coreutils runtime cell with the exact
      observed result.
