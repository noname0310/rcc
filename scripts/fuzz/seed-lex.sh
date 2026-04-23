#!/usr/bin/env bash
# scripts/fuzz/seed-lex.sh --- populate fuzz/corpus/lex/ from c-testsuite.
#
# Task: tasks/03-lex/12-fuzz-target.md.
#
# Used by Linux / macOS developers and by CI. Mirrors
# scripts/fuzz/seed-lex.ps1 line-for-line in terms of the curated set so
# both platforms converge on the same corpus.
#
# Invariants:
#   * Idempotent: re-running overwrites the same file set.
#   * Small: each seed file is < 1 KiB; the entire corpus is < 4 KiB.
#   * Self-contained: only reads from the vendored c-testsuite, no net.

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"

SRC_DIR="${REPO_ROOT}/third_party/testsuites/c-testsuite/tests/single-exec"
DST_DIR="${REPO_ROOT}/fuzz/corpus/lex"

if [[ ! -d "${SRC_DIR}" ]]; then
    echo "error: c-testsuite not vendored at ${SRC_DIR}" >&2
    echo "hint: run 'cargo xtask fetch-testsuites --only c-testsuite'" >&2
    exit 2
fi

mkdir -p "${DST_DIR}"

# Curated seeds — chosen for lexical diversity (hello world, pointers,
# preprocessor directives, string literals, forward decls, char
# escapes). Keep this list small; libFuzzer mutates aggressively.
SEEDS=(
    00001  # bare main returning 0
    00002  # constant return value
    00003  # simple declaration + return
    00005  # nested pointers, dereference chain
    00011  # chained assignment
    00012  # comma operator
    00023  # sizeof expression
    00061  # #define directive (preprocessor tokens)
    00094  # bitwise operators
    00098  # tiny expression
    00112  # string literal vs null pointer comparison
    00114  # forward declaration + function definition
)

for stem in "${SEEDS[@]}"; do
    src="${SRC_DIR}/${stem}.c"
    dst="${DST_DIR}/${stem}.c"
    if [[ ! -f "${src}" ]]; then
        echo "warn: seed ${src} not found, skipping" >&2
        continue
    fi
    cp -f -- "${src}" "${dst}"
done

count=$(find "${DST_DIR}" -maxdepth 1 -type f -name '*.c' | wc -l | tr -d ' ')
echo "seeded ${count} files into ${DST_DIR}"
