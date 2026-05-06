# 12 — curl probe plan

## Source snapshot

- Project: curl (HTTP/HTTPS client and `libcurl`)
- Upstream URL: <https://github.com/curl/curl>
- Clone command: `git clone --depth=1 https://github.com/curl/curl.git upstream`
- Resolved commit: `9c9a4f3eabbb6f24277538d28a00afa25ba2839a`
- Local probe source (the tree the WSL probe ran against):
  `~/work-curl-rcc-20260505/curl/`
- Date fetched: 2026-05-06
- Wrapper source policy: a checked-in wrapper should clone the upstream tree
  into ignored `upstream/`. The wrapper must not edit upstream `.c` or `.h`
  files. All adaptation belongs in wrapper scripts, environment variables, and
  CMake flags.

## Why this project

curl is a much larger and more heterogeneous code base than the earlier real-
world probes (sqlite-amalgamation is one big TU; curl is ~184 .c files plus a
CLI sub-target). The probe exercises:

- a real CMake-driven multi-target build with `rcc` as `CMAKE_C_COMPILER`,
  including `try_compile` runs and feature probes;
- broad GNU/POSIX header surface through `--linux-gnu-hosted`-equivalent
  flags (`-D_GNU_SOURCE`, the full `-fgnu-*` compatibility set);
- many cross-file mutual `#include` chains via `curl/curl.h` ↔ `curl/multi.h`
  and similar headers;
- 60+ `extern const struct Foo bar;` declarations with forward-only struct
  decls (`Curl_easyopts`, `Curl_protocol_*`, `Curl_ssl_*`, `Curl_cft_*`);
- real system-header typedef redefinitions (`uint8_t` from stdint.h vs
  netinet/in.h);
- a static archive (`libcurl.a`) link followed by a CLI executable link
  (`src/curl`);
- HTTP runtime smoke against the public `example.com` and `httpbin.org`
  endpoints — a network-positive smoke that catches link-only and runtime-
  initialisation regressions invisible to the conformance suites.

## Probe command

```sh
bash real_world/projects/12-curl/scripts/run-cli-smoke.sh
```

The script uses an existing `target/debug/rcc` when present. If missing, it
builds `rcc` with LLVM support first. Set `RCC_BUILD=1` to force a rebuild or
`RCC_BUILD=0` to require an existing `RCC`/`target/debug/rcc` binary.

The script honours these environment overrides:

| Variable | Purpose | Default |
| --- | --- | --- |
| `CURL_SRC` | Upstream curl source tree | `${HOME}/work-curl-rcc-20260505/curl` |
| `RCC` | rcc binary | `${REPO_ROOT}/target/debug/rcc` |
| `LLVM_SYS_181_PREFIX` | LLVM install prefix | `/usr/lib/llvm-18` |
| `RCC_LINKER_DRIVER` | Linker invoked by rcc | `/usr/bin/clang-18` |
| `RCC_TIMEOUT` | Per-rcc-invocation timeout | `600s` |
| `MAKEFLAGS_J` | `make -j` parallelism | `4` |
| `NETWORK_SMOKE` | Run HTTP smoke (1) or skip (0) | `1` |

Equivalent command sequence from the repository root is recorded in
`PROJECT.md` (CMake configure + `make` + a `--version` and an `example.com`
HTTP smoke).

## Baseline oracle

- Host compiler: `clang-18` or `gcc` building the same upstream tree with the
  same disable list, then running the same smoke. The baseline confirms the
  smoke is not sensitive to network conditions or the disable list.
- Expected `--version` line: `curl 8.20.1-DEV (Linux) libcurl/8.20.1-DEV`.
- Expected `example.com` GET: HTTP `200`, body length `528`, body begins
  `<!doctype html>`.
- Expected `google.com` redirect (no `-L`): HTTP `301`, `Location` header
  starts with `http://www.google.com/`.

## rcc probe

- `rcc` invoked as the C compiler by CMake; flags listed in `PROJECT.md`.
- Final link is performed by `clang-18` via `RCC_LINKER_DRIVER`.
- Run command:
  - `build/src/curl --version`
  - `build/src/curl http://example.com/`
- Expected comparison: identical exit status (`0`), identical body (`example.com`
  serves a static page), identical `--version` first-line tag.

## Allowed local adaptation

- Wrapper scripts:
  - `scripts/run-cli-smoke.sh`
- Generated files:
  - `build/`
  - `artifacts/`
  - `logs/`
- Local ignored source probe:
  - future checked-in wrapper source under project-local ignored `upstream/`
  - current ad-hoc probe at WSL `~/work-curl-rcc-20260505/curl/`
- CMake disable flags listed in `PROJECT.md`.

## Disallowed adaptation checklist

- [x] No upstream `.c` file modified
- [x] No upstream `.h` file modified
- [x] No curl runtime body stubbed out
- [x] Runtime smoke is a real HTTP request, not a host-mocked stub
- [x] No host `cc` fallback for any `lib/*.c` or `src/*.c` translation unit

## Failure log

| ID | Command | Symptom | Classification | Follow-up status |
| --- | --- | --- | --- | --- |
| CURL-001 | `make` (any `lib/*.c.o` target) | `[E0021] recursive include cycle while loading curl/curl.h` for every translation unit that pulls `curl/curl.h` (which `#include`s `curl/multi.h`, which `#include`s `curl/curl.h`) | preprocessor cycle detection runs before the include-guard fingerprint is published, so cross-file mutual inclusion through valid `#ifndef` guards is rejected | fixed in `crates/rcc_preprocess/src/include.rs` (ad-hoc guard scan on cycle entry; silent skip when guard symbol is already defined) |
| CURL-002 | `rcc lib/bufref.c` (and 60+ other `lib/*.c.o` targets) | `rcc: failed to lower HIR type TyId(N) to LLVM: type TyId(N) has no compile-time layout: record has no fields or completed layout` | LLVM codegen requires a complete struct layout to lower the storage type of an `extern const struct Foo bar;`, even though the symbol is consumed only by pointer | fixed in `crates/rcc_codegen_llvm/src/lib.rs` (extend the incomplete-array external-global `[0 x i8]` placeholder path to incomplete records via new helper `is_incomplete_record`) |
| CURL-003 | `rcc lib/protocol.c` | `[E0088] typed HIR invariant violation: typedef def#NNNN type contains Ty::Error` against `Curl_bufq_writer`, `Curl_bufq_reader`, and four function declarations in `lib/bufq.h`. Triggered when the same TU sees stdint.h's `typedef unsigned char uint8_t;` and netinet/in.h's `typedef __uint8_t uint8_t;` | file-scope typedef registration overwrites `resolver.ordinary[name]` to the most recent def; pass-2 finalises typedef slots in source order, so uses interleaved with the later redef resolve through the still-`tcx.error` slot | fixed in `crates/rcc_hir_lower/src/lib.rs` (do not overwrite the resolver binding when the existing entry is already a typedef def) |

## rcc flags

Common options used by every translation unit:

```text
-fgnu-named-variadic -fgnu-va-args-elision -fgnu-permissive-paste
-fgnu-attributes -fgnu-typeof -fgnu-alignof -fgnu-statement-expressions
-fgnu-omitted-conditional-operand -fgnu-conditional-void-operand
-fgnu-range-designators -fgnu-case-ranges -fgnu-labels-as-values
-fgnu-lvalue-comma -fgnu-pragma-pack -fgnu-function-names
-fgnu-va-area -fgnu-builtin-libcalls
-std=c99 -D_GNU_SOURCE -fvisibility=hidden -DNDEBUG
```

The CMake disable list (`CURL_USE_*=OFF`, `CURL_DISABLE_*=ON`) is intentional:
the probe targets a runnable HTTP-only CLI without external linkage, so any
TLS, compression, libssh2, libidn2, libpsl, or HTTP/2 dependency is excluded
both at configure time and at link time.

## Exit criteria

- [x] All 184 `lib/*.c` files reach a successful `.o` (modulo features
      excluded by the CMake disable list)
- [x] `lib/libcurl.a` is created
- [x] `src/*.c` files reach a successful `.o`
- [x] `src/curl` is linked by `${RCC_LINKER_DRIVER}`
- [x] `build/src/curl --version` prints the curl version line
- [x] `build/src/curl http://example.com/` returns HTTP 200 and 528 bytes
- [x] No upstream `.c` or `.h` was edited
- [x] Three compiler findings (`CURL-001`, `CURL-002`, `CURL-003`) have
      checked-in fixes in the rcc tree
- [x] `RESULTS.md` updated to pass
