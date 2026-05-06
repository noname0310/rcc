# rcc v0.0.0 Release Notes

## Supported Surface

- Hosted C99/C11 compiler targeting `x86_64-unknown-linux-gnu` for the current
  M7 release surface.
- LLVM 18 backend with hosted libc and external clang-compatible linker tooling.
- `cargo install rcc-compiler` installs an executable named `rcc`.

## Conformance Snapshot

See [`conformance.md`](conformance.md) for the release dashboard and xfail
policy. Required C99/C11 failures are treated as compiler bugs even when
aggregate pass rates look healthy.

## Known Non-goals

- Windows target support.
- Bundled libc, glibc, musl, or MSVCRT implementation.
- Native linker implementation.
- Treating exploratory GNU/TinyCC-specific failures as strict ISO release
  failures.

## Stability

This `0.0.0` upload validates the crates.io and `cargo install` distribution
path. It is intentionally below `1.0.0` because fuzzing and conformance work are
still active.
