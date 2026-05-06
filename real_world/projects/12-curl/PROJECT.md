# 12 — curl

Status: reproducible PASS probe — the checked-in wrapper fetches the pinned
upstream `curl/curl` tree (commit `9c9a4f3`) into this project's ignored
`upstream/` directory, builds both a host-compiler baseline and an `rcc-llvm`
CMake tree, links a static `libcurl.a` plus the `curl` CLI executable, and
compares both binaries against a local HTTP oracle.

Source: <https://github.com/curl/curl>

The wrapper clones the upstream tree into ignored `upstream/` by default and
keeps adaptation in this directory as wrapper scripts only. Do not edit
upstream `.c` or `.h` files.

## Reproducible wrapper

```sh
bash real_world/projects/12-curl/scripts/run-cli-smoke.sh
```

The script writes the CMake build tree, the linked CLI, logs, and smoke-test
output under this project's ignored `build/`, `logs/`, and `artifacts/`
directories. The latest recorded result is in `RESULTS.md`.

## Observed command sequence

Run from a Linux host with `git`, `cmake`, `make`, `python3`, `clang-18`, and
the `rcc` build prerequisites available. The probe builds against the curl tree
at `${CURL_SRC}` (default: `real_world/projects/12-curl/upstream`). If the
default source tree is absent, the wrapper fetches `${CURL_REV}` from
`${CURL_URL}`.

Rcc + linker environment:

```sh
export LLVM_SYS_181_PREFIX=/usr/lib/llvm-18
export RCC_LINKER_DRIVER=/usr/bin/clang-18
export RCC=$(realpath target/debug/rcc)
```

GNU compatibility flag set required by the upstream tree:

```sh
GNU_FLAGS="-fgnu-named-variadic -fgnu-va-args-elision -fgnu-permissive-paste \
  -fgnu-attributes -fgnu-typeof -fgnu-alignof -fgnu-statement-expressions \
  -fgnu-omitted-conditional-operand -fgnu-conditional-void-operand \
  -fgnu-range-designators -fgnu-case-ranges -fgnu-labels-as-values \
  -fgnu-lvalue-comma -fgnu-pragma-pack -fgnu-function-names -fgnu-va-area \
  -fgnu-builtin-libcalls"
```

Configure (TLS / compression / non-HTTP protocols disabled, threaded resolver
disabled, IPv6 disabled, manual disabled, static build):

```sh
cmake \
  -DCMAKE_C_COMPILER=$RCC \
  -DCMAKE_C_FLAGS="$GNU_FLAGS -std=c99 -D_GNU_SOURCE -fvisibility=hidden" \
  -DCMAKE_C_FLAGS_RELEASE="-DNDEBUG" \
  -DCMAKE_BUILD_TYPE=Release \
  -DBUILD_SHARED_LIBS=OFF \
  -DCMAKE_DISABLE_FIND_PACKAGE_Threads=TRUE \
  -DCURL_USE_OPENSSL=OFF -DCURL_USE_LIBSSH2=OFF -DCURL_USE_LIBPSL=OFF \
  -DCURL_ZLIB=OFF -DCURL_BROTLI=OFF -DCURL_ZSTD=OFF \
  -DUSE_LIBIDN2=OFF -DUSE_NGHTTP2=OFF \
  -DENABLE_THREADED_RESOLVER=OFF -DENABLE_UNIX_SOCKETS=OFF -DENABLE_IPV6=OFF \
  -DCURL_DISABLE_LDAP=ON -DCURL_DISABLE_LDAPS=ON \
  -DCURL_DISABLE_FTP=ON -DCURL_DISABLE_SMTP=ON -DCURL_DISABLE_IMAP=ON \
  -DCURL_DISABLE_POP3=ON -DCURL_DISABLE_GOPHER=ON -DCURL_DISABLE_RTSP=ON \
  -DCURL_DISABLE_TELNET=ON -DCURL_DISABLE_TFTP=ON -DCURL_DISABLE_DICT=ON \
  -DCURL_DISABLE_FILE=ON -DCURL_DISABLE_MQTT=ON -DCURL_DISABLE_PROXY=ON \
  -DCURL_DISABLE_HTTP_AUTH=ON -DCURL_DISABLE_KERBEROS_AUTH=ON \
  -DCURL_DISABLE_NEGOTIATE_AUTH=ON \
  -DCURL_DISABLE_ALTSVC=ON -DCURL_DISABLE_HSTS=ON -DCURL_DISABLE_WEBSOCKETS=ON \
  -DBUILD_TESTING=OFF -DBUILD_CURL_EXE=ON -DENABLE_CURL_MANUAL=OFF \
  "${CURL_SRC}"
```

Build:

```sh
make -j4
```

Smoke tests:

```sh
build/rcc-cmake/src/curl --version
build/rcc-cmake/src/curl -sS -o artifacts/rcc-local.html \
  -w 'status=%{http_code} size=%{size_download}\n' \
  http://127.0.0.1:<local-port>/
```

Expected local HTTP stdout/body: identical to the host-compiler curl binary
built from the same source and CMake disable list. Set `NETWORK_SMOKE=1` to
also run the optional public `example.com` request.

## Compiler findings fixed by the probe

| ID | Classification | Status |
| --- | --- | --- |
| CURL-001 | Cross-file mutual `#include` cycle (`curl/curl.h` ↔ `curl/multi.h`) emits E0021 even when both headers carry canonical `#ifndef` guards, because the include-guard cache only fingerprints a header *after* its first inclusion completes. | fixed in `crates/rcc_preprocess/src/include.rs`: ad-hoc guard scan on cycle entry; if the guard symbol is already defined, silently skip the recursive edge. |
| CURL-002 | LLVM type lowering rejects `extern const struct Foo bar;` when `struct Foo` was only forward-declared in this translation unit (e.g. `Curl_easyopts`, `Curl_protocol_*`, `Curl_ssl_*`, `Curl_cft_*`). curl's CMake inputs reference 60+ such externs through `urldata.h`. | fixed in `crates/rcc_codegen_llvm/src/lib.rs`: extend the existing incomplete-array external-global placeholder (`[0 x i8]`) to incomplete records. New helper `is_incomplete_record` gates the path. |
| CURL-003 | File-scope typedef redefinition with the same name (e.g. stdint.h's `typedef unsigned char uint8_t;` followed by netinet/in.h's `typedef __uint8_t uint8_t;`) overwrites `resolver.ordinary[name]` to point at the second def. Pass-2 typedef finalisation runs in source order, so any use of `uint8_t` between the two definitions resolves to a slot still holding the placeholder `tcx.error`, and the cascading `Ty::Error` is reported as E0088 against earlier typedefs (`Curl_bufq_writer`, `Curl_bufq_reader`, `Curl_bufq_pass`, ...). | fixed in `crates/rcc_hir_lower/src/lib.rs`: when registering a file-scope typedef, do not overwrite an existing resolver entry that already names a typedef def. The duplicate def is still created and finalised so `find_file_scope_ordinary_def` continues to see both. |

## Important option notes

- `CURL_USE_OPENSSL=OFF` and the other `*_DISABLE_*` flags above are required
  because the probe targets a runnable HTTP-only CLI without external linkage
  (no OpenSSL, libssh2, zlib, etc.). Re-enabling any of these would bring in
  large external dependencies that are out of scope for the rcc probe and would
  obscure rcc-only failures.
- `CMAKE_DISABLE_FIND_PACKAGE_Threads=TRUE` is required because the build is
  intentionally single-threaded; libcurl normally probes pthread, and removing
  the probe is more reliable than relying on absent libpthread linkage.
- `-fvisibility=hidden` is silently ignored by `rcc` (compatibility flag) but is
  retained because curl's CMake passes it unconditionally.
- `BUILD_TESTING=OFF` and `ENABLE_CURL_MANUAL=OFF` keep the build scope narrow.
  The runtime smoke is a real HTTP request, not curl's internal test suite.

Runtime ownership: every `lib/*.c` and `src/*.c` body in the `rcc` curl CLI is
compiled by `rcc`; libc/libdl/libm behaviour and process startup come from the
host clang-18 driver invoked as the linker. The host binary exists only as the
real-world oracle required by `real_world/README.md`.
