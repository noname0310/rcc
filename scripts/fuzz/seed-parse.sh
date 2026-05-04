#!/usr/bin/env bash
# scripts/fuzz/seed-parse.sh --- populate fuzz/corpus/parse/.
#
# The parse fuzzer wants translation-unit shaped inputs, not just token
# fragments. This seed set mixes standalone c-testsuite programs with
# chibicc parser-heavy fixtures.

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"

CTEST_DIR="${REPO_ROOT}/third_party/testsuites/c-testsuite/tests/single-exec"
CHIBICC_DIR="${REPO_ROOT}/third_party/testsuites/chibicc/test"
DST_DIR="${REPO_ROOT}/fuzz/corpus/parse"

mkdir -p "${DST_DIR}"

copy_seed() {
    local src="$1"
    local name="$2"
    if [[ -f "${src}" ]]; then
        cp -f -- "${src}" "${DST_DIR}/${name}"
    else
        echo "warn: seed ${src} not found, skipping" >&2
    fi
}

for name in 00001.c 00002.c 00003.c 00005.c 00011.c 00012.c 00023.c 00061.c 00094.c 00098.c 00112.c 00114.c; do
    copy_seed "${CTEST_DIR}/${name}" "ctest-${name}"
done

for name in arith.c cast.c control.c decl.c enum.c function.c initializer.c struct.c typedef.c union.c; do
    copy_seed "${CHIBICC_DIR}/${name}" "chibicc-${name}"
done

count=$(find "${DST_DIR}" -maxdepth 1 -type f | wc -l | tr -d ' ')
echo "seeded ${count} files into ${DST_DIR}"
