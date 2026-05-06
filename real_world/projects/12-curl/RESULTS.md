# curl Results

Last verified: 2026-05-06 on Linux/WSL with LLVM 18 (clang-18 driving the link).

Command:

```sh
bash real_world/projects/12-curl/scripts/run-cli-smoke.sh
```

Equivalent manual command sequence is recorded in `PROJECT.md` and `plan.md`.

Result:

- `rcc` build availability: success
- wrapper source acquisition: success; default source lives under ignored
  `real_world/projects/12-curl/upstream/`
- host baseline CMake build: success
- `cmake` configure with `rcc` as `CMAKE_C_COMPILER`: success
- `lib/*.c` translation units: 183 / 184 emitted (one `lib/curlx/*.c` file is
  excluded by the CMake disable list); zero `rcc` errors
- `src/*.c` translation units (CLI front-end): success
- `lib/libcurl.a` archive: success (2,269,244 bytes)
- `src/curl` final link: success (2,239,608 bytes)
- runtime `--version` smoke: success
- runtime local HTTP smoke: success; `rcc` output/body match the host-compiler
  baseline
- optional runtime HTTP smoke against `http://example.com/`: success in the
  latest manual network run (HTTP 200, 528 B)

Upstream curl tree probed: commit `9c9a4f3eabbb6f24277538d28a00afa25ba2839a`
of <https://github.com/curl/curl>, no `.c`/`.h` modifications.

## Runtime evidence

`build/rcc-cmake/src/curl --version`:

```text
curl 8.20.1-DEV (Linux) libcurl/8.20.1-DEV
Release-Date: [unreleased]
Protocols: http ipfs ipns
Features: Largefile
```

Local loopback HTTP smoke:

```text
host: status=200 size=149
rcc:  status=200 size=149
body diff: empty
```

`build/rcc-cmake/src/curl http://example.com/` (optional network smoke):

```text
status=200 size=528 time=0.087360
first 80 bytes: <!doctype html><html lang="en"><head><title>Example Domain</title><meta name="vi
```

`build/rcc-cmake/src/curl http://google.com/` redirect handling:

```text
no -L:  status=301 location=http://www.google.com/
with -L: final_url=http://www.google.com/ status=200 hops=1
```

`build/rcc-cmake/src/curl -I http://example.com/`:

```text
HTTP/1.1 200 OK
Date: Wed, 06 May 2026 11:33:51 GMT
Content-Type: text/html
Connection: keep-alive
Server: cloudflare
Last-Modified: Fri, 01 May 2026 03:27:47 GMT
Allow: GET, HEAD
Accept-Ranges: bytes
```

`build/rcc-cmake/src/curl -X POST -d 'foo=bar&baz=qux' http://httpbin.org/post`:

```text
status=200
{
  "args": {},
  "data": "",
  "files": {},
  "form": {
    "baz": "qux",
    "foo": "bar"
  },
  ...
```

(The httpbin endpoint echoes `Content-Type: application/x-www-form-urlencoded`,
`Content-Length: 15`, `User-Agent: curl/8.20.1-DEV`, and the parsed form
fields, confirming the rcc-built CLI produced a well-formed request body.)

## Build artefact fingerprints

```text
sha256(libcurl.a) = ec3a26e9acd319d950f4d4d6c80e1beb13bddf302c82de380d92eebe52f99311
sha256(src/curl)  = f334a9aa8ab72fcfebff915748fe55ec1246ad9d06395c41eb08c12de500fdc6
size(libcurl.a)   = 2269244
size(src/curl)    = 2239608
```

## Compiler bugs found

| ID | Status | Symptom |
| --- | --- | --- |
| CURL-001 | fixed | Cross-file mutual `#include` of `curl/curl.h` ↔ `curl/multi.h` rejected with E0021 even when both headers carry canonical `#ifndef` guards (guard cache only published after first inclusion completes) |
| CURL-002 | fixed | `extern const struct Foo bar;` rejected by LLVM type lowering when `struct Foo` is forward-declared only (60+ such symbols across `urldata.h`-included headers: `Curl_easyopts`, `Curl_protocol_*`, `Curl_ssl_*`, `Curl_cft_*`) |
| CURL-003 | fixed | File-scope typedef redefinition with the same name (`uint8_t` from stdint.h then again from netinet/in.h) caused E0088 against earlier dependent typedefs (`Curl_bufq_writer` and friends in `lib/bufq.h`) because the resolver was overwritten to the still-`tcx.error` second def while pass-2 finalisation runs in source order |

## Important option result

The passing CLI probe disables every TLS, compression, non-HTTP protocol, and
GNU-extension authentication option. It is the smallest CMake configuration
that still builds a runnable curl CLI and serves a real HTTP request. Any
deviation (re-enabling OpenSSL, libssh2, IDN, libpsl, or HTTP/2) brings in
external linkage outside the rcc probe's scope and would mask rcc-only
findings behind external-toolchain noise.

`CMAKE_DISABLE_FIND_PACKAGE_Threads=TRUE` is required because the build is
intentionally single-threaded; `ENABLE_THREADED_RESOLVER=OFF` is the dependent
runtime switch.

## Upstream source policy

The wrapper does not modify upstream C or header files. By default it fetches
the official upstream tree into this project directory's ignored `upstream/`
subdirectory, with generated outputs kept under the ignored `build/`, `logs/`,
or `artifacts/` directories.
