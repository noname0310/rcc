# Vendored test-suite licenses

This directory collects the **upstream** licenses of every external test
suite fetched by `cargo xtask fetch-testsuites`. The licenses apply only to
the contents of `third_party/testsuites/<suite>/`; they do **not** extend to
the rcc compiler sources.

Suites pinned by `third_party/MANIFEST.toml`:

| Suite | License |
|-------|---------|
| `c-testsuite` | MIT |
| `chibicc` | MIT |
| `gcc-torture` | GPL-3.0-or-later WITH GCC-exception-3.1 |
| `tcc-tests2` | LGPL-2.1 |
| `llvm-test-suite` | Apache-2.0 WITH LLVM-exception |
| `csmith` | BSD-2-Clause |

GPL-licensed suites (`gcc-torture`, `tcc-tests2`) are **not** fetched by
default and are **not** linked into any rcc binary. They are executed as
separate processes only, and results are collected through stdout/exit
codes — see `crates/rcc_conformance/`.

**Note on GPL test isolation:** GPL-gated test programs are compiled and
run as independent executables by the rcc conformance harness. No GPL
source code is ever compiled into, linked with, or distributed alongside
the rcc compiler. The `--include-gpl` flag on `cargo xtask fetch-testsuites`
must be passed explicitly to download these suites.

When a suite is fetched, `cargo xtask fetch-testsuites` will copy its license
file into `LICENSES/<suite>.txt` for easy review.
