#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "${script_dir}/.." && pwd)"
repo_root="$(cd "${project_dir}/../../.." && pwd)"
upstream="${project_dir}/upstream"

if [[ ! -d "${upstream}/.git" ]]; then
  git clone -c core.autocrlf=false --depth 1 https://github.com/libtom/libtommath.git "${upstream}"
fi
git -C "${upstream}" config core.autocrlf false

mkdir -p "${project_dir}/build/host" \
         "${project_dir}/build/rcc" \
         "${project_dir}/artifacts" \
         "${project_dir}/logs" \
         "${project_dir}/scratch"

cat > "${project_dir}/scratch/libtommath_smoke.c" <<'C_EOF'
#include <stdio.h>
#include "tommath.h"

int main(void) {
    mp_int a, b, c;
    char buf[128];

    if (mp_init_multi(&a, &b, &c, NULL) != MP_OKAY) return 1;
    if (mp_read_radix(&a, "12345678901234567890", 10) != MP_OKAY) return 2;
    if (mp_read_radix(&b, "987654321", 10) != MP_OKAY) return 3;
    if (mp_mul(&a, &b, &c) != MP_OKAY) return 4;
    if (mp_to_radix(&c, buf, sizeof(buf), NULL, 10) != MP_OKAY) return 5;

    puts(buf);
    mp_clear_multi(&a, &b, &c, NULL);
    return 0;
}
C_EOF

cd "${upstream}"
find . -maxdepth 1 \( -name 'mp_*.c' -o -name 's_*.c' \) -printf '%f\n' | sort > ../logs/sources.txt
mapfile -t sources < ../logs/sources.txt

host_cc="${HOST_CC:-gcc}"
"${host_cc}" -std=c99 -Wall -Wextra -I. "${sources[@]}" ../scratch/libtommath_smoke.c \
  -o ../build/host/libtommath_smoke \
  > ../logs/host-libtommath-smoke.stdout \
  2> ../logs/host-libtommath-smoke.stderr
../build/host/libtommath_smoke > ../artifacts/host-libtommath-smoke.stdout

rcc_bin="${RCC:-${repo_root}/target/release/rcc}"
if [[ ! -x "${rcc_bin}" ]]; then
  LLVM_SYS_181_PREFIX="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}" \
    cargo build --release -p rcc_driver --bin rcc --features llvm \
    --manifest-path "${repo_root}/Cargo.toml"
fi

rm -f ../build/rcc/libtommath_smoke
rcc_jobs="${RCC_JOBS:-8}"
rcc_timeout="${RCC_TIMEOUT:-240s}"

LLVM_SYS_181_PREFIX="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}" \
RCC_LINKER_DRIVER="${RCC_LINKER_DRIVER:-clang-18}" \
  timeout "${rcc_timeout}" "${rcc_bin}" -j "${rcc_jobs}" --std=c99 -Wall -I. \
  "${sources[@]}" ../scratch/libtommath_smoke.c \
  -o ../build/rcc/libtommath_smoke \
  > ../logs/rcc-libtommath-smoke.stdout \
  2> ../logs/rcc-libtommath-smoke.stderr

../build/rcc/libtommath_smoke > ../artifacts/rcc-libtommath-smoke.stdout
diff -u ../artifacts/host-libtommath-smoke.stdout ../artifacts/rcc-libtommath-smoke.stdout

echo "libtommath smoke: host and rcc outputs match"
