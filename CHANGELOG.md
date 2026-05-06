# Changelog

## 0.0.0

- Initial crates.io pipeline release for the `rcc-compiler` package.
- Publishes the internal `rcc_*` crate graph needed by `cargo install
  rcc-compiler`.
- Installs an executable named `rcc`; the default feature enables the LLVM
  backend.
- This is not a 1.0.0 stability release. Fuzzing and conformance work remain
  active.
