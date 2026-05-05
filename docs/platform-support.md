# Platform Support Matrix

This page describes the release surface for `rcc` as of M7. It separates
front-end target modelling from executable support: parsing a target triple or
emitting LLVM IR for it is not the same as supporting that target as a hosted
binary output.

## Release Support

| area | supported | notes |
| --- | --- | --- |
| Host OS for development | Linux x86-64, WSL2 x86-64, Windows x86-64 | Linux/WSL is the primary release path. Windows host support is for building and running tests, including LLVM-C dynamic loading setup. |
| Release executable target | `x86_64-unknown-linux-gnu` | This is the only target with link+run conformance and performance baselines. |
| Front-end/layout target models | `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, `x86_64-pc-windows-msvc` | These triples drive predefined macros and C data-layout answers. They are not all link+run release targets. |
| LLVM version | LLVM 18.1.x | Inkwell is built with `llvm18-1`; other LLVM majors are not part of this release. |
| Linker strategy | clang-compatible driver with `-fuse-ld=lld` | `rcc` emits objects; final executable linking is delegated to a clang-style driver so CRT startup files and hosted libc paths remain platform-owned. |
| C runtime / libc | host libc | `rcc` does not implement libc, glibc, musl, MSVCRT, or C headers. Tests declare the few libc functions they call or use host headers only in preprocessor modes. |
| Native linker | external LLVM/system tools | `rcc` does not implement a native linker. |

## Tool Discovery

The driver discovers the external tools it needs in this order:

| purpose | override env vars | fallback names |
| --- | --- | --- |
| clang-compatible linker driver | `RCC_LINKER_DRIVER`, `RCC_CLANG`, `CLANG` | `clang`, `clang-18`, `clang-17` |
| LLVM lld | `RCC_LLD`, `LLD` | Linux/WSL: `ld.lld`, `ld.lld-18`, `lld`; Windows: `lld-link`, `ld.lld`, `lld` |
| LLVM prefix | `RCC_LLVM_PREFIX`, `LLVM_SYS_181_PREFIX`, `LLVM_SYS_180_PREFIX`, `LLVM_PREFIX` | none |
| object inspection | `RCC_OBJDUMP`, then LLVM prefix lookup | `llvm-objdump`, `llvm-objdump-18`, `objdump` |

Inspection commands:

```text
rcc --version --verbose
rcc --print-search-dirs
```

Both commands are info-only and do not require an input file. Missing tools are
reported inline with the env var and searched names/paths, so setup problems are
actionable before a compile attempt.

## Linux / WSL Setup

Install LLVM 18 and a host C toolchain, then build the LLVM-enabled driver:

```sh
sudo apt-get update
sudo apt-get install -y clang-18 lld-18 llvm-18 llvm-18-dev
LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 \
  cargo build --release --bin rcc --features rcc_codegen_llvm/llvm
```

Hello-world smoke:

```sh
cat > /tmp/rcc-hello.c <<'C'
int printf(const char *, ...);
int main(void) {
    printf("hello\n");
    return 0;
}
C

LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 \
  target/release/rcc /tmp/rcc-hello.c -O2 -o /tmp/rcc-hello
/tmp/rcc-hello
```

Expected output:

```text
hello
```

Stage smoke commands:

```sh
target/release/rcc --version --verbose
target/release/rcc --print-search-dirs
target/release/rcc --emit=llvm-ir -o /tmp/rcc-hello.ll /tmp/rcc-hello.c
target/release/rcc -c -o /tmp/rcc-hello.o /tmp/rcc-hello.c
target/release/rcc -O2 -o /tmp/rcc-hello /tmp/rcc-hello.c && /tmp/rcc-hello
```

## Windows Host Setup

Windows host builds that enable the LLVM backend use the official LLVM 18
Windows archive layout and dynamic `LLVM-C.dll` loading:

```powershell
$env:LLVM_SYS_181_PREFIX = 'D:\Tools\clang+llvm-18.1.8-x86_64-pc-windows-msvc'
$env:PATH = "$env:LLVM_SYS_181_PREFIX\bin;$env:PATH"
cargo test -p rcc_codegen_llvm --features llvm-windows-llvm-c
```

The required layout is:

```text
<prefix>\lib\LLVM-C.lib
<prefix>\bin\LLVM-C.dll
```

This is Windows **host** support. It does not mean `rcc` can produce and run
Windows/MSVC executables as a release target. `x86_64-pc-windows-msvc` currently
exists as a target model for layout/predefined macros and for future backend
work.

## Unsupported In This Release

- Windows/MSVC executable target support.
- macOS link+run release support.
- AArch64 link+run release support.
- Cross-linking against non-host CRT/libc installations.
- A built-in standard library, libc implementation, or native linker.
