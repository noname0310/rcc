#!/usr/bin/env bash
# scripts/fuzz/seed-preprocess.sh --- populate fuzz/corpus/preprocess/ from
# the vendored chibicc suite.
#
# Task: tasks/04-preprocess/19-fuzz-target.md.
#
# Used by Linux / macOS developers and by CI. Mirrors
# scripts/fuzz/seed-preprocess.ps1 line-for-line in terms of the curated
# set so both platforms converge on the same corpus.
#
# Invariants:
#   * Idempotent: re-running overwrites the same file set.
#   * Small: total corpus is < 16 KiB (macro.c dominates at ~6.5 KiB).
#   * Self-contained: only reads from the vendored chibicc suite, no
#     network access.

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"

SRC_DIR="${REPO_ROOT}/third_party/testsuites/chibicc/test"
DST_DIR="${REPO_ROOT}/fuzz/corpus/preprocess"

if [[ ! -d "${SRC_DIR}" ]]; then
    echo "error: chibicc suite not vendored at ${SRC_DIR}" >&2
    echo "hint: run 'cargo xtask fetch-testsuites --only chibicc'" >&2
    exit 2
fi

mkdir -p "${DST_DIR}"

# Curated seeds — chosen for preprocessor diversity (typedef + header
# include, the full macro corpus, #line, #pragma once, common symbol
# declarations, small compat / extern / offsetof surface). The sibling
# .h files give libFuzzer a template for header-shaped inputs so
# mutations around `#include "..."` start from realistic content.
#
# Keep this list small; libFuzzer mutates aggressively.
SEEDS=(
    typedef.c       # 486 B  — typedef forms + '#include "test.h"'
    macro.c         # 6.5 KiB — full chibicc macro corpus (GNU ext OK)
    line.c          # 357 B  — '#line' directive
    pragma-once.c   # 119 B  — '#pragma once'
    const.c         # 306 B  — small TU, predefined macros
    commonsym.c     # 264 B  — tentative defs + comments
    compat.c        # 396 B  — pragma pack + misc attrs
    extern.c        # 351 B  — extern + forward decls
    offsetof.c      # 284 B  — stddef-style header usage
    include1.h      # 114 B  — header chained via "include2.h"
    include2.h      #  19 B  — terminal header in the chain
)

for name in "${SEEDS[@]}"; do
    src="${SRC_DIR}/${name}"
    dst="${DST_DIR}/${name}"
    if [[ ! -f "${src}" ]]; then
        echo "warn: seed ${src} not found, skipping" >&2
        continue
    fi
    cp -f -- "${src}" "${dst}"
done

count=$(find "${DST_DIR}" -maxdepth 1 -type f | wc -l | tr -d ' ')
echo "seeded ${count} files into ${DST_DIR}"
