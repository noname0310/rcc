#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "${script_dir}/.." && pwd)"
repo_root="$(cd "${project_dir}/../../.." && pwd)"
upstream="${project_dir}/upstream"

if [[ ! -d "${upstream}/.git" ]]; then
  git clone --depth 1 https://github.com/benhoyt/inih.git "${upstream}"
fi

mkdir -p "${project_dir}/build/host" \
         "${project_dir}/build/rcc" \
         "${project_dir}/artifacts" \
         "${project_dir}/logs"

cd "${upstream}/tests"

host_cc="${HOST_CC:-gcc}"
"${host_cc}" -std=c99 -Wall ../ini.c unittest.c -o ../../build/host/unittest_multi
../../build/host/unittest_multi > ../../artifacts/host-unittest-multi.stdout
diff -u baseline_multi.txt ../../artifacts/host-unittest-multi.stdout

rcc_bin="${RCC:-${repo_root}/target/release/rcc}"
if [[ ! -x "${rcc_bin}" ]]; then
  LLVM_SYS_181_PREFIX="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}" \
    cargo build --release -p rcc_driver --bin rcc --features rcc_codegen_llvm/llvm \
    --manifest-path "${repo_root}/Cargo.toml"
fi

LLVM_SYS_181_PREFIX="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}" \
RCC_LINKER_DRIVER="${RCC_LINKER_DRIVER:-clang-18}" \
  "${rcc_bin}" --std=c99 -Wall ../ini.c unittest.c -o ../../build/rcc/unittest_multi \
  > ../../logs/rcc-unittest-multi.stdout \
  2> ../../logs/rcc-unittest-multi.stderr

../../build/rcc/unittest_multi > ../../artifacts/rcc-unittest-multi.stdout
diff -u baseline_multi.txt ../../artifacts/rcc-unittest-multi.stdout

echo "inih unittest_multi: host and rcc outputs match baseline"
