#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "${script_dir}/.." && pwd)"
repo_root="$(cd "${project_dir}/../../.." && pwd)"
upstream="${project_dir}/upstream"

if [[ ! -d "${upstream}/.git" ]]; then
  git clone -c core.autocrlf=false --depth 1 https://github.com/madler/zlib.git "${upstream}"
fi
git -C "${upstream}" config core.autocrlf false

mkdir -p "${project_dir}/build/host" \
         "${project_dir}/build/rcc" \
         "${project_dir}/artifacts" \
         "${project_dir}/logs" \
         "${project_dir}/scratch"

cat > "${project_dir}/scratch/zlib_smoke.c" <<'C_EOF'
#include <stdio.h>
#include <string.h>
#include "zlib.h"

int main(void) {
    const unsigned char input[] = "hello from rcc zlib smoke";
    unsigned char compressed[256];
    unsigned char output[256];
    uLongf compressed_len = sizeof(compressed);
    uLongf output_len = sizeof(output);
    int err;

    err = compress(compressed, &compressed_len, input, (uLong)sizeof(input));
    if (err != Z_OK) {
        fprintf(stderr, "compress failed: %d\n", err);
        return 1;
    }

    memset(output, 0, sizeof(output));
    err = uncompress(output, &output_len, compressed, compressed_len);
    if (err != Z_OK) {
        fprintf(stderr, "uncompress failed: %d\n", err);
        return 2;
    }
    if (output_len != sizeof(input) || memcmp(output, input, sizeof(input)) != 0) {
        fprintf(stderr, "roundtrip mismatch\n");
        return 3;
    }

    puts("zlib smoke ok");
    return 0;
}
C_EOF

cd "${upstream}"

core_sources=(
  adler32.c
  crc32.c
  deflate.c
  infback.c
  inffast.c
  inflate.c
  inftrees.c
  trees.c
  zutil.c
  compress.c
  uncompr.c
)

host_cc="${HOST_CC:-gcc}"
"${host_cc}" -std=c99 -Wall "${core_sources[@]}" ../scratch/zlib_smoke.c \
  -I. -o ../build/host/zlib_smoke \
  > ../logs/host-zlib-smoke.stdout \
  2> ../logs/host-zlib-smoke.stderr
../build/host/zlib_smoke > ../artifacts/host-zlib-smoke.stdout

rcc_bin="${RCC:-${repo_root}/target/release/rcc}"
if [[ ! -x "${rcc_bin}" ]]; then
  LLVM_SYS_181_PREFIX="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}" \
    cargo build --release -p rcc_driver --bin rcc --features rcc_codegen_llvm/llvm \
    --manifest-path "${repo_root}/Cargo.toml"
fi

LLVM_SYS_181_PREFIX="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}" \
RCC_LINKER_DRIVER="${RCC_LINKER_DRIVER:-clang-18}" \
  "${rcc_bin}" --std=c99 -Wall "${core_sources[@]}" ../scratch/zlib_smoke.c \
  -I. -o ../build/rcc/zlib_smoke \
  > ../logs/rcc-zlib-smoke.stdout \
  2> ../logs/rcc-zlib-smoke.stderr

../build/rcc/zlib_smoke > ../artifacts/rcc-zlib-smoke.stdout
diff -u ../artifacts/host-zlib-smoke.stdout ../artifacts/rcc-zlib-smoke.stdout

echo "zlib smoke: host and rcc outputs match"
