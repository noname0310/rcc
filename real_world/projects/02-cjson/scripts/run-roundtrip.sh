#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "${script_dir}/.." && pwd)"
repo_root="$(cd "${project_dir}/../../.." && pwd)"
upstream="${project_dir}/upstream"

if [[ ! -d "${upstream}/.git" ]]; then
  git clone --depth 1 https://github.com/DaveGamble/cJSON.git "${upstream}"
fi

mkdir -p "${project_dir}/build/host" \
         "${project_dir}/build/rcc" \
         "${project_dir}/artifacts" \
         "${project_dir}/logs" \
         "${project_dir}/scratch"

cat > "${project_dir}/scratch/roundtrip.c" <<'C_EOF'
#include <stdio.h>
#include <string.h>
#include "cJSON.h"

int main(void) {
    cJSON *root = cJSON_Parse("{\"name\":\"rcc\",\"answer\":42}");
    cJSON *answer = 0;
    char *printed = 0;

    if (root == 0) {
        return 1;
    }
    answer = cJSON_GetObjectItemCaseSensitive(root, "answer");
    if (!cJSON_IsNumber(answer) || answer->valueint != 42) {
        cJSON_Delete(root);
        return 2;
    }
    printed = cJSON_PrintUnformatted(root);
    if (printed == 0) {
        cJSON_Delete(root);
        return 3;
    }
    printf("%s\n", printed);
    cJSON_free(printed);
    cJSON_Delete(root);
    return 0;
}
C_EOF

cd "${upstream}"

host_cc="${HOST_CC:-gcc}"
"${host_cc}" -std=c99 -Wall cJSON.c ../scratch/roundtrip.c -I. -lm -o ../build/host/roundtrip
../build/host/roundtrip > ../artifacts/host-roundtrip.stdout

rcc_bin="${RCC:-${repo_root}/target/release/rcc}"
if [[ ! -x "${rcc_bin}" ]]; then
  LLVM_SYS_181_PREFIX="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}" \
    cargo build --release -p rcc_driver --bin rcc --features rcc_codegen_llvm/llvm \
    --manifest-path "${repo_root}/Cargo.toml"
fi

LLVM_SYS_181_PREFIX="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}" \
RCC_LINKER_DRIVER="${RCC_LINKER_DRIVER:-clang-18}" \
  "${rcc_bin}" --std=c99 -Wall cJSON.c ../scratch/roundtrip.c -I. -lm -o ../build/rcc/roundtrip \
  > ../logs/rcc-roundtrip.stdout \
  2> ../logs/rcc-roundtrip.stderr

../build/rcc/roundtrip > ../artifacts/rcc-roundtrip.stdout
diff -u ../artifacts/host-roundtrip.stdout ../artifacts/rcc-roundtrip.stdout

echo "cJSON roundtrip: host and rcc outputs match"
