# rcc v0.0.1 Release Notes

## Supported Surface

- Hosted C99 compiler targeting `x86_64-unknown-linux-gnu` for the M7 release.
- LLVM 18 backend with hosted libc and external clang-compatible linker tooling.

## Conformance Snapshot

See [`conformance.md`](conformance.md) for the frozen release dashboard and xfail policy.

## Known Non-goals

- Windows target support.
- Bundled libc/glibc/MSVCRT implementation.
- Native linker implementation.
- Treating exploratory GNU/C11 suite failures as strict C99 release failures.
