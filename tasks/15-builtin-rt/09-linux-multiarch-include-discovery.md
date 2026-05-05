> ✓ done — 2026-05-05

# 15-09: Linux GNU multiarch include discovery

**Phase:** 15-builtin-rt    **Depends on:** 15-07    **Milestone:** real-world-01

## Goal
Make hosted Linux probes find target multiarch headers such as
`/usr/include/x86_64-linux-gnu/bits/wordsize.h` without manual `--isystem`.

## Scope
- In: Debian/Ubuntu GNU multiarch include directory names for supported Linux
  targets, searched before the raw LLVM triple directory.
- Out: libc implementation, non-Linux SDK discovery, and C++ include search.

## Acceptance
- Linux x86-64 sysroot candidate order includes both
  `usr/include/x86_64-linux-gnu` and `usr/include/x86_64-unknown-linux-gnu`.
- AArch64 GNU target candidates include `usr/include/aarch64-linux-gnu`.

## Real-world trigger
`real_world/projects/01-inih` reached glibc `features-time64.h`, which includes
`bits/wordsize.h` and `bits/timesize.h` through the host multiarch include dir.

