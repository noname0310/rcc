#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "${script_dir}/.." && pwd)"
repo_root="$(cd "${project_dir}/../../.." && pwd)"
upstream="${project_dir}/upstream"

lua_version="5.5.0"
archive="/tmp/lua-${lua_version}.tar.gz"
sha256="57ccc32bbbd005cab75bcc52444052535af691789dba2b9016d5c50640d68b3d"

if [[ ! -f "${upstream}/src/Makefile" ]]; then
  mkdir -p "${upstream}"
  curl -L -R -o "${archive}" "https://www.lua.org/ftp/lua-${lua_version}.tar.gz"
  echo "${sha256}  ${archive}" | sha256sum -c -
  tar -xzf "${archive}" --strip-components=1 -C "${upstream}"
fi

mkdir -p "${project_dir}/build/host" \
         "${project_dir}/build/rcc" \
         "${project_dir}/artifacts" \
         "${project_dir}/logs" \
         "${project_dir}/scratch"

cd "${upstream}/src"

core_o="lapi.o lcode.o lctype.o ldebug.o ldo.o ldump.o lfunc.o lgc.o llex.o lmem.o lobject.o lopcodes.o lparser.o lstate.o lstring.o ltable.o ltm.o lundump.o lvm.o lzio.o"
lib_o="lauxlib.o lbaselib.o lcorolib.o ldblib.o liolib.o lmathlib.o loadlib.o loslib.o lstrlib.o ltablib.o lutf8lib.o linit.o"
sources=()
for obj in ${core_o} ${lib_o}; do
  sources+=("${obj%.o}.c")
done
printf "%s\n" "${sources[@]}" > "${project_dir}/logs/base-sources.txt"

host_cc="${HOST_CC:-gcc}"
"${host_cc}" -std=c99 -Wall -Wextra -DLUA_USE_JUMPTABLE=0 -DLUA_NOBUILTIN -I. \
  "${sources[@]}" lua.c -lm \
  -o "${project_dir}/build/host/lua" \
  > "${project_dir}/logs/host-lua-smoke.stdout" \
  2> "${project_dir}/logs/host-lua-smoke.stderr"

"${project_dir}/build/host/lua" -e 'print(_VERSION); print(6*7)' \
  > "${project_dir}/artifacts/host-lua-smoke.stdout"

rcc_bin="${RCC:-${repo_root}/target/release/rcc}"
if [[ ! -x "${rcc_bin}" ]]; then
  LLVM_SYS_181_PREFIX="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}" \
    cargo build --release -p rcc_driver --bin rcc --features llvm \
    --manifest-path "${repo_root}/Cargo.toml"
fi

LLVM_SYS_181_PREFIX="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}" \
RCC_LINKER_DRIVER="${RCC_LINKER_DRIVER:-clang-18}" \
  timeout "${RCC_TIMEOUT:-300s}" "${rcc_bin}" -j "${RCC_JOBS:-8}" --std=c99 -Wall \
  -DLUA_USE_JUMPTABLE=0 -DLUA_NOBUILTIN -I. \
  "${sources[@]}" lua.c -lm \
  -o "${project_dir}/build/rcc/lua" \
  > "${project_dir}/logs/rcc-lua-smoke.stdout" \
  2> "${project_dir}/logs/rcc-lua-smoke.stderr"

"${project_dir}/build/rcc/lua" -v > "${project_dir}/artifacts/rcc-lua-version.stdout"

set +e
"${project_dir}/build/rcc/lua" -e 'print(_VERSION); print(6*7)' \
  > "${project_dir}/artifacts/rcc-lua-smoke.stdout" \
  2> "${project_dir}/artifacts/rcc-lua-smoke.stderr"
rcc_status=$?
set -e

if [[ "${rcc_status}" -ne 0 ]]; then
  echo "lua smoke blocked: rcc-built interpreter failed to execute a trivial chunk" >&2
  cat "${project_dir}/artifacts/rcc-lua-smoke.stderr" >&2
  exit "${rcc_status}"
fi

diff -u "${project_dir}/artifacts/host-lua-smoke.stdout" \
        "${project_dir}/artifacts/rcc-lua-smoke.stdout"

echo "lua smoke: host and rcc outputs match"
