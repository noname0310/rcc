#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "${script_dir}/.." && pwd)"
repo_root="$(cd "${project_dir}/../../.." && pwd)"

sqlite_src="${SQLITE_SRC:-${repo_root}/sqlite/sqlite-amalgamation-3530000}"
build_dir="${project_dir}/build"
logs_dir="${project_dir}/logs"
artifacts_dir="${project_dir}/artifacts"

rcc_bin="${RCC:-${repo_root}/target/debug/rcc}"
host_cc="${HOST_CC:-cc}"
llvm_prefix="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}"

mkdir -p "${build_dir}" "${logs_dir}" "${artifacts_dir}"

if [[ ! -f "${sqlite_src}/sqlite3.c" ]] || [[ ! -f "${sqlite_src}/shell.c" ]]; then
    echo "SQLite amalgamation source is incomplete: ${sqlite_src}" >&2
    echo "Set SQLITE_SRC to a directory containing sqlite3.c and shell.c." >&2
    exit 2
fi

rcc_build_mode="${RCC_BUILD:-auto}"
case "${rcc_build_mode}" in
    1 | true | yes)
        should_build_rcc=1
        ;;
    0 | false | no)
        should_build_rcc=0
        ;;
    auto)
        if [[ -x "${rcc_bin}" ]]; then
            should_build_rcc=0
        else
            should_build_rcc=1
        fi
        ;;
    *)
        echo "Unsupported RCC_BUILD mode: ${rcc_build_mode}" >&2
        echo "Use RCC_BUILD=auto, RCC_BUILD=1, or RCC_BUILD=0." >&2
        exit 2
        ;;
esac

if [[ "${should_build_rcc}" -eq 1 ]]; then
    (
        cd "${repo_root}"
        LLVM_SYS_181_PREFIX="${llvm_prefix}" \
            cargo build -p rcc_driver --bin rcc --features llvm
    ) >"${logs_dir}/cargo-build-rcc.stdout" \
     2>"${logs_dir}/cargo-build-rcc.stderr"
fi

common_flags=(
    --linux-gnu-hosted
    --std=c11
    -w
    -DSQLITE_THREADSAFE=0
    -DSQLITE_OMIT_LOAD_EXTENSION
    -DSQLITE_OMIT_PROGRESS_CALLBACK
    -DSQLITE_OMIT_SHARED_CACHE
    -DSQLITE_DEFAULT_MEMSTATUS=0
)

sqlite_obj="${build_dir}/sqlite3.rcc.o"
shell_obj="${build_dir}/shell.rcc.o"
cli_bin="${build_dir}/sqlite3.rcc"

LLVM_SYS_181_PREFIX="${llvm_prefix}" timeout "${RCC_TIMEOUT:-600s}" "${rcc_bin}" \
    "${sqlite_src}/sqlite3.c" \
    -c -o "${sqlite_obj}" \
    "${common_flags[@]}" \
    >"${logs_dir}/rcc-sqlite3.stdout" \
    2>"${logs_dir}/rcc-sqlite3.stderr"

LLVM_SYS_181_PREFIX="${llvm_prefix}" timeout "${RCC_TIMEOUT:-600s}" "${rcc_bin}" \
    "${sqlite_src}/shell.c" \
    -c -o "${shell_obj}" \
    -I "${sqlite_src}" \
    "${common_flags[@]}" \
    >"${logs_dir}/rcc-shell.stdout" \
    2>"${logs_dir}/rcc-shell.stderr"

"${host_cc}" "${sqlite_obj}" "${shell_obj}" \
    -o "${cli_bin}" \
    -ldl -lm \
    >"${logs_dir}/host-link.stdout" \
    2>"${logs_dir}/host-link.stderr"

printf 'CREATE TABLE t(x); INSERT INTO t VALUES(1); SELECT * FROM t;\n' \
    | "${cli_bin}" :memory: \
    >"${artifacts_dir}/sqlite-cli-smoke.stdout" \
    2>"${artifacts_dir}/sqlite-cli-smoke.stderr"

grep -qx '1' "${artifacts_dir}/sqlite-cli-smoke.stdout"

cat "${artifacts_dir}/sqlite-cli-smoke.stdout"
